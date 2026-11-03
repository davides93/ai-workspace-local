use crate::{arg_str, Risk, Tool, ToolContext, ToolOutput};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct WebFetchTool;

/// Redirects followed by `web_fetch`; each hop is vetted again.
const MAX_REDIRECTS: usize = 5;
/// Time budget of one fetch, redirects included.
const FETCH_TIMEOUT: Duration = Duration::from_secs(25);

/// A URL that passed the network policy, with the addresses its host resolved to. The caller
/// connects to exactly these addresses, so the DNS answer that was checked is the one used (no
/// second lookup for an attacker to rebind). Empty for IP literals and when the policy does not
/// look at addresses.
#[derive(Debug, Clone)]
pub struct Vetted {
    pub url: reqwest::Url,
    pub addrs: Vec<SocketAddr>,
}

/// Rejects non-HTTP schemes, private/loopback targets (unless allowed) and hosts outside the allowlist.
pub async fn check_url(url: &str, ctx: &ToolContext) -> Result<reqwest::Url> {
    Ok(vet_url(url, ctx).await?.url)
}

/// [`check_url`] plus the vetted addresses, resolving names with the system resolver.
pub async fn vet_url(url: &str, ctx: &ToolContext) -> Result<Vetted> {
    vet_url_with(url, ctx, system_resolve, proxy_configured()).await
}

async fn system_resolve(host: String, port: u16) -> std::io::Result<Vec<IpAddr>> {
    let found = tokio::time::timeout(Duration::from_secs(8), tokio::net::lookup_host((host.as_str(), port)))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout DNS"))??;
    Ok(found.map(|s| s.ip()).collect())
}

/// Is an HTTP(S) proxy configured in the environment? Then the proxy resolves host names itself.
fn proxy_configured() -> bool {
    ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .any(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty()))
}

/// `via_proxy`: a proxy will resolve the host name, so a lookup that fails here is not fatal.
async fn vet_url_with<R, F>(url: &str, ctx: &ToolContext, resolve: R, via_proxy: bool) -> Result<Vetted>
where
    R: Fn(String, u16) -> F,
    F: Future<Output = std::io::Result<Vec<IpAddr>>>,
{
    let u = reqwest::Url::parse(url).map_err(|e| Error::Invalid(format!("URL non valido: {e}")))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(Error::Denied("solo URL http/https".into()));
    }
    let host = u.host_str().ok_or_else(|| Error::Invalid("URL senza host".into()))?.to_lowercase();
    if !ctx.allow_domains.is_empty() && !ctx.allow_domains.iter().any(|d| host == *d || host.ends_with(&format!(".{d}"))) {
        return Err(Error::Denied(format!("{host} non è tra i domini consentiti")));
    }
    if ctx.allow_private_network {
        return Ok(Vetted { url: u, addrs: vec![] });
    }
    let local = || Error::Denied(format!("{host} è un indirizzo locale o privato: abilita la rete locale per questo agente"));
    // `host_str` keeps the brackets of an IPv6 literal (`[::1]`), which do not parse as an address.
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return if is_blocked_ip(&ip) { Err(local()) } else { Ok(Vetted { url: u, addrs: vec![] }) };
    }
    let name = bare.trim_end_matches('.');
    if name == "localhost" || name.ends_with(".localhost") {
        return Err(local());
    }
    let port = u.port_or_known_default().unwrap_or(80);
    let ips = match resolve(name.to_string(), port).await {
        Ok(ips) if !ips.is_empty() => ips,
        // A proxy that resolves names on its own is the only reason to go on without an answer.
        _ if via_proxy => return Ok(Vetted { url: u, addrs: vec![] }),
        Ok(_) => return Err(Error::Runtime(format!("impossibile risolvere {host}"))),
        Err(e) => return Err(Error::Runtime(format!("impossibile risolvere {host}: {e}"))),
    };
    // Every address counts: a name with one public and one private record is refused.
    if ips.iter().any(is_blocked_ip) {
        return Err(local());
    }
    Ok(Vetted { url: u, addrs: ips.into_iter().map(|ip| SocketAddr::new(ip, port)).collect() })
}

/// Addresses a model-driven fetch must never reach: loopback, private ranges, link-local (cloud
/// metadata at 169.254.169.254 included), CGNAT, unique-local IPv6, multicast, reserved and
/// documentation ranges, and the IPv6 forms that embed an IPv4 address.
pub fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => blocked_v4(*v),
        IpAddr::V6(v) => blocked_v6(v),
    }
}

fn blocked_v4(v: Ipv4Addr) -> bool {
    let o = v.octets();
    v.is_loopback() // 127.0.0.0/8
        || v.is_private() // 10/8, 172.16/12, 192.168/16
        || v.is_link_local() // 169.254/16
        || v.is_multicast() // 224/4
        || o[0] == 0 // 0.0.0.0/8 ("this network"; 0.0.0.0 reaches localhost on many systems)
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64/10 CGNAT
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24 IETF protocol assignments
        || (o[0] == 192 && o[1] == 0 && o[2] == 2) // TEST-NET-1
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18/15 benchmarking
        || (o[0] == 198 && o[1] == 51 && o[2] == 100) // TEST-NET-2
        || (o[0] == 203 && o[1] == 0 && o[2] == 113) // TEST-NET-3
        || o[0] >= 240 // 240/4 reserved and the broadcast address
}

fn blocked_v6(v: &Ipv6Addr) -> bool {
    let s = v.segments();
    let embedded = |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    // ::/96 (unspecified, loopback, IPv4-compatible) and ::ffff:0:0/96 (IPv4-mapped): an IPv4
    // address in disguise, never a legitimate public target.
    if s[..5] == [0; 5] && (s[5] == 0 || s[5] == 0xffff) {
        return true;
    }
    match s[0] {
        // NAT64: the well-known prefix embeds an IPv4 address, the local-use one is internal.
        0x0064 if s[1] == 0xff9b => s[2] == 1 || blocked_v4(embedded(s[6], s[7])),
        // 100::/64 discard-only.
        0x0100 => s[1] == 0 && s[2] == 0 && s[3] == 0,
        // 6to4 embeds the IPv4 address in bits 16..48.
        0x2002 => blocked_v4(embedded(s[1], s[2])),
        // Teredo (2001::/32) and documentation (2001:db8::/32).
        0x2001 => s[1] == 0 || s[1] == 0x0db8,
        a if (a & 0xfe00) == 0xfc00 => true,                           // fc00::/7 unique local
        a if (a & 0xffc0) == 0xfe80 || (a & 0xffc0) == 0xfec0 => true, // link-local and deprecated site-local
        a if (a & 0xff00) == 0xff00 => true,                           // multicast
        _ => false,
    }
}

/// GET `start` following redirects by hand (at most [`MAX_REDIRECTS`]): every hop goes through
/// `vet` again, so a public page cannot bounce the request to `http://127.0.0.1:…` or to the cloud
/// metadata address, and the connection goes to the addresses `vet` checked. Returns the final
/// response and its URL.
async fn fetch_following<V, F>(start: &str, vet: V) -> Result<(reqwest::Response, reqwest::Url)>
where
    V: Fn(String) -> F,
    F: Future<Output = Result<Vetted>>,
{
    let deadline = Instant::now() + FETCH_TIMEOUT;
    let mut next = start.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let Vetted { url, addrs } = vet(next).await?;
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Error::Runtime("timeout della richiesta".into()));
        }
        let mut builder = reqwest::Client::builder()
            .timeout(left)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Mozilla/5.0 (compatible; AIWorkspaceLocal/0.1; +https://github.com/davides93/ai-workspace-local)");
        if let (Some(host), false) = (url.host_str(), addrs.is_empty()) {
            builder = builder.resolve_to_addrs(host, &addrs);
        }
        let res = builder.build().map_err(Error::other)?.get(url.clone()).send().await.map_err(|e| Error::Runtime(e.to_string()))?;
        if !res.status().is_redirection() {
            return Ok((res, url));
        }
        let Some(location) = res.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()) else {
            // A 3xx without a target (304, 300…) is a final answer.
            return Ok((res, url));
        };
        next = url.join(location).map_err(|e| Error::Invalid(format!("reindirizzamento non valido: {e}")))?.to_string();
    }
    Err(Error::Runtime(format!("troppi reindirizzamenti (più di {MAX_REDIRECTS})")))
}

/// Very small HTML → text conversion: drops scripts/styles, keeps block structure and link targets.
pub fn html_to_text(html: &str) -> (Option<String>, String) {
    let title = regex::Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap().captures(html).map(|c| decode(c[1].trim()));
    let mut s = regex::Regex::new(r"(?is)<(script|style|noscript|svg|head)[^>]*>.*?</(script|style|noscript|svg|head)>")
        .unwrap()
        .replace_all(html, " ")
        .to_string();
    s = regex::Regex::new(r#"(?is)<a\s[^>]*href="(https?://[^"]+)"[^>]*>(.*?)</a>"#).unwrap().replace_all(&s, "$2 ($1)").to_string();
    s = regex::Regex::new(r"(?i)<(br|/p|/div|/li|/h[1-6]|/tr|/section|/article|/header|/footer)[^>]*>")
        .unwrap()
        .replace_all(&s, "\n")
        .to_string();
    s = regex::Regex::new(r"(?i)<li[^>]*>").unwrap().replace_all(&s, "\n• ").to_string();
    s = regex::Regex::new(r"(?s)<[^>]+>").unwrap().replace_all(&s, " ").to_string();
    s = decode(&s);
    let lines: Vec<String> = s.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    (title, lines.join("\n"))
}

fn decode(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Scarica una pagina web pubblica e la restituisce come testo (titolo, contenuto, link)."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"url":{"type":"string"},"max_chars":{"type":"integer","description":"Default 20000"}},"required":["url"]})
    }
    fn risk(&self) -> Risk {
        Risk::Network
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let start = arg_str(&args, "url")?;
        let max = args.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(20_000).clamp(500, 200_000) as usize;
        let (res, url) = tokio::select! {
            r = fetch_following(start, |u| async move { vet_url(&u, ctx).await }) => r?,
            _ = ctx.cancel.cancelled() => return Err(Error::Cancelled),
        };
        let status = res.status();
        let ctype = res.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let (body, cut) = read_capped(res, MAX_BODY_BYTES).await?;
        let (title, text) = if ctype.contains("html") || body.trim_start().starts_with('<') { html_to_text(&body) } else { (None, body) };
        let mut out = format!("URL: {url}\nStato: {status}\n");
        if let Some(t) = title {
            out.push_str(&format!("Titolo: {t}\n"));
        }
        out.push('\n');
        let truncated = text.chars().count() > max;
        out.push_str(&text.chars().take(max).collect::<String>());
        if truncated {
            out.push_str("\n… [troncato]");
        }
        if cut {
            out.push_str(&format!("\n[risposta oltre {} MB: letta solo la parte iniziale]", MAX_BODY_BYTES >> 20));
        }
        Ok(if status.is_success() { ToolOutput::text(out) } else { ToolOutput::error(out) })
    }
}

/// Upper bound for a fetched body: a page that streams forever (or a multi-GB file behind an
/// innocent URL) must not exhaust the memory of the app.
const MAX_BODY_BYTES: usize = 5 << 20;

/// Reads at most `cap` bytes of the body; the flag says whether the rest was dropped.
async fn read_capped(mut res: reqwest::Response, cap: usize) -> Result<(String, bool)> {
    let mut buf: Vec<u8> = Vec::new();
    let mut cut = false;
    while let Some(chunk) = res.chunk().await.map_err(|e| Error::Runtime(e.to_string()))? {
        let room = cap - buf.len();
        if chunk.len() >= room {
            buf.extend_from_slice(&chunk[..room]);
            cut = chunk.len() > room || res.chunk().await.ok().flatten().is_some();
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), cut))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn bodies_are_capped() {
        let (port, _) = serve(|path| if path == "/big" { ok(&"a".repeat(3000)) } else { ok("piccolo") }).await;
        let get = |p: &str| reqwest::get(format!("http://127.0.0.1:{port}{p}"));
        let (body, cut) = read_capped(get("/big").await.unwrap(), 1000).await.unwrap();
        assert_eq!((body.len(), cut), (1000, true));
        let (body, cut) = read_capped(get("/small").await.unwrap(), 1000).await.unwrap();
        assert_eq!((body.as_str(), cut), ("piccolo", false));
    }

    #[test]
    fn converts_html_to_readable_text() {
        let (t, body) = html_to_text("<html><head><title>Orari &amp; servizi</title><style>x{}</style></head><body><h1>Anagrafe</h1><p>Lun-Ven 9-12</p><ul><li>Uno</li><li>Due</li></ul><a href=\"https://x.it/a\">Dettagli</a><script>alert(1)</script></body></html>");
        assert_eq!(t.as_deref(), Some("Orari & servizi"));
        assert!(body.contains("Anagrafe") && body.contains("Lun-Ven 9-12") && body.contains("• Uno"));
        assert!(body.contains("Dettagli (https://x.it/a)"));
        assert!(!body.contains("alert"));
    }

    #[tokio::test]
    async fn blocks_private_and_non_http_targets() {
        let ctx = ToolContext::new("r", std::env::temp_dir());
        assert!(check_url("http://127.0.0.1:7777/api", &ctx).await.is_err());
        assert!(check_url("http://192.168.1.10/", &ctx).await.is_err());
        assert!(check_url("file:///etc/passwd", &ctx).await.is_err());
        let mut lan = ctx.clone();
        lan.allow_private_network = true;
        assert!(check_url("http://192.168.1.10/", &lan).await.is_ok());
        let mut allow = ctx.clone();
        allow.allow_domains = vec!["example.org".into()];
        assert!(check_url("https://evil.com/", &allow).await.is_err());
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn blocked_addresses() {
        for a in [
            // IPv4
            "0.0.0.0",
            "0.1.2.3",
            "127.0.0.1",
            "127.255.255.254",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254", // cloud metadata
            "169.254.0.1",
            "100.64.0.1", // CGNAT
            "100.100.100.200",
            "100.127.255.255",
            "192.0.0.192",
            "192.0.2.1",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.7",
            "203.0.113.9",
            "224.0.0.251",
            "240.0.0.1",
            "255.255.255.255",
            // IPv6
            "::",
            "::1",
            "::ffff:127.0.0.1", // IPv4-mapped
            "::ffff:8.8.8.8",   // mapped even when the IPv4 side is public
            "::127.0.0.1",      // IPv4-compatible
            "::8.8.8.8",
            "fc00::1", // unique local
            "fd12:3456:789a::1",
            "fe80::1", // link-local
            "febf::1",
            "fec0::1",
            "ff02::1",            // multicast
            "64:ff9b::7f00:1",    // NAT64 of 127.0.0.1
            "64:ff9b::a9fe:a9fe", // NAT64 of 169.254.169.254
            "64:ff9b:1::1",
            "2002:7f00:1::1",   // 6to4 of 127.0.0.1
            "2002:c0a8:101::1", // 6to4 of 192.168.1.1
            "2001::1",          // Teredo
            "2001:db8::1",
            "100::1",
        ] {
            assert!(is_blocked_ip(&ip(a)), "{a} must be blocked");
        }
        for a in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "172.15.255.255",
            "172.32.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "198.17.0.1",
            "198.20.0.1",
            "223.255.255.255",
            "2606:4700:4700::1111",
            "2a00:1450:4001:81b::200e",
            "64:ff9b::808:808", // NAT64 of 8.8.8.8
            "2002:808:808::1",  // 6to4 of 8.8.8.8
        ] {
            assert!(!is_blocked_ip(&ip(a)), "{a} must be allowed");
        }
    }

    #[tokio::test]
    async fn parses_bracketed_ipv6_literals() {
        let ctx = ToolContext::new("r", std::env::temp_dir());
        for u in [
            "http://[::1]:7777/api/kill",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]:7777/",
            "http://[::ffff:7f00:1]/",
            "http://[fe80::1]/",
            "http://[fd00::1234]:8080/",
            "http://[64:ff9b::7f00:1]/",
            "http://[::]/",
            // Other spellings of the same loopback address are normalised by the URL parser.
            "http://2130706433/",
            "http://0x7f.0.0.1/",
            "http://0177.0.0.1/",
            "http://127.1/",
            "http://0.0.0.0:7777/",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.64.0.1/",
        ] {
            let e = check_url(u, &ctx).await.expect_err(u);
            assert!(matches!(e, Error::Denied(_)), "{u}: {e}");
        }
        assert!(check_url("http://[2606:4700:4700::1111]/", &ctx).await.is_ok());
        assert!(check_url("http://8.8.8.8/", &ctx).await.is_ok());
        let mut lan = ctx.clone();
        lan.allow_private_network = true;
        assert!(check_url("http://[::1]:7777/", &lan).await.is_ok());
    }

    #[tokio::test]
    async fn localhost_names_are_blocked_without_a_lookup() {
        let ctx = ToolContext::new("r", std::env::temp_dir());
        let never = |_: String, _: u16| async {
            if true {
                panic!("no lookup for localhost names");
            }
            Ok::<Vec<IpAddr>, std::io::Error>(vec![])
        };
        for u in ["http://localhost/", "http://LOCALHOST:7777/", "http://localhost./", "http://api.localhost/", "http://a.b.localhost:1/"] {
            assert!(matches!(vet_url_with(u, &ctx, never, false).await, Err(Error::Denied(_))), "{u}");
        }
    }

    #[tokio::test]
    async fn every_resolved_address_is_checked_and_pinned() {
        let ctx = ToolContext::new("r", std::env::temp_dir());
        let answer = |ips: Vec<&'static str>| {
            move |_: String, _: u16| {
                let ips = ips.clone();
                async move { Ok(ips.into_iter().map(ip).collect::<Vec<_>>()) }
            }
        };
        // A name that resolves to a loopback or private address is refused...
        for ips in [vec!["127.0.0.1"], vec!["192.168.1.9"], vec!["::1"], vec!["169.254.169.254"], vec!["::ffff:10.0.0.1"]] {
            let r = vet_url_with("https://rebind.example/", &ctx, answer(ips.clone()), false).await;
            assert!(matches!(r, Err(Error::Denied(_))), "{ips:?}: {r:?}");
            // A proxy does not change that: the answer we did get is still checked.
            let r = vet_url_with("https://rebind.example/", &ctx, answer(ips.clone()), true).await;
            assert!(matches!(r, Err(Error::Denied(_))), "{ips:?} via proxy: {r:?}");
        }
        // ...also when only one of its records is (a public record next to a private one).
        let r = vet_url_with("https://rebind.example/", &ctx, answer(vec!["93.184.216.34", "10.0.0.5"]), false).await;
        assert!(matches!(r, Err(Error::Denied(_))), "{r:?}");
        let r = vet_url_with("https://rebind.example/", &ctx, answer(vec!["10.0.0.5", "93.184.216.34"]), false).await;
        assert!(matches!(r, Err(Error::Denied(_))), "{r:?}");
        // A public name comes back with the addresses to connect to, on the URL's port.
        let public = answer(vec!["93.184.216.34", "2606:2800:220:1::1"]);
        let ok = vet_url_with("https://public.example:8443/x", &ctx, public, false).await.unwrap();
        assert_eq!(ok.addrs, vec![SocketAddr::new(ip("93.184.216.34"), 8443), SocketAddr::new(ip("2606:2800:220:1::1"), 8443)]);
        let http = vet_url_with("http://public.example/", &ctx, answer(vec!["93.184.216.34"]), false).await.unwrap();
        assert_eq!(http.addrs[0].port(), 80);
        // Allowing the private network skips the lookup altogether.
        let mut lan = ctx.clone();
        lan.allow_private_network = true;
        assert!(vet_url_with("http://nas.example/", &lan, answer(vec!["10.0.0.5"]), false).await.unwrap().addrs.is_empty());
    }

    #[tokio::test]
    async fn dns_failure_is_not_a_pass() {
        let ctx = ToolContext::new("r", std::env::temp_dir());
        let fail = |_: String, _: u16| async { Err::<Vec<IpAddr>, _>(std::io::Error::other("nxdomain")) };
        let empty = |_: String, _: u16| async { Ok::<Vec<IpAddr>, std::io::Error>(vec![]) };
        // Without a proxy to resolve for us, a name we cannot resolve must not go through...
        assert!(matches!(vet_url_with("https://nxdomain.example/", &ctx, fail, false).await, Err(Error::Runtime(_))));
        assert!(matches!(vet_url_with("https://nxdomain.example/", &ctx, empty, false).await, Err(Error::Runtime(_))));
        // ...with one, the proxy does the resolving and the request goes to it unpinned.
        let via = vet_url_with("https://nxdomain.example/", &ctx, fail, true).await.unwrap();
        assert!(via.addrs.is_empty());
    }

    /// Minimal HTTP server: `handler(path)` returns the raw response for each request.
    async fn serve(handler: impl Fn(&str) -> String + Send + Sync + 'static) -> (u16, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let (h, handler) = (hits.clone(), Arc::new(handler));
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let (h, handler) = (h.clone(), handler.clone());
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    h.fetch_add(1, Ordering::SeqCst);
                    let _ = s.write_all(handler(&path).as_bytes()).await;
                });
            }
        });
        (port, hits)
    }

    fn ok(body: &str) -> String {
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    fn redirect(to: &str) -> String {
        format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    /// The test server stands in for a public site: the real policy applies to everything else.
    fn stand_in(public_port: u16, ctx: ToolContext) -> impl Fn(String) -> std::pin::Pin<Box<dyn Future<Output = Result<Vetted>>>> {
        move |u: String| {
            let ctx = ctx.clone();
            Box::pin(async move {
                if u.starts_with(&format!("http://127.0.0.1:{public_port}/")) {
                    return Ok(Vetted { url: reqwest::Url::parse(&u).unwrap(), addrs: vec![] });
                }
                vet_url(&u, &ctx).await
            })
        }
    }

    #[tokio::test]
    async fn redirect_to_a_private_address_is_refused_at_the_hop() {
        let (secret_port, secret_hits) = serve(|_| ok("segreto interno")).await;
        let to_secret = format!("http://127.0.0.1:{secret_port}/api/secret");
        let (public_port, public_hits) = serve(move |p| match p {
            "/to-loopback" => redirect(&to_secret),
            "/to-metadata" => redirect("http://169.254.169.254/latest/meta-data/"),
            "/to-v6-loopback" => redirect("http://[::1]:9/"),
            "/to-mapped" => redirect("http://[::ffff:127.0.0.1]:9/"),
            "/relative" => redirect("/final"),
            "/final" => ok("pagina finale"),
            _ => ok("ciao"),
        })
        .await;
        let ctx = ToolContext::new("r", std::env::temp_dir());
        let base = format!("http://127.0.0.1:{public_port}");

        for path in ["/to-loopback", "/to-metadata", "/to-v6-loopback", "/to-mapped"] {
            let r = fetch_following(&format!("{base}{path}"), stand_in(public_port, ctx.clone())).await;
            assert!(matches!(r, Err(Error::Denied(_))), "{path}: {:?}", r.map(|(_, u)| u));
        }
        assert_eq!(secret_hits.load(Ordering::SeqCst), 0, "the private server was never contacted");
        assert_eq!(public_hits.load(Ordering::SeqCst), 4);

        // A redirect to an allowed place is followed (relative Location included).
        let (res, url) = fetch_following(&format!("{base}/relative"), stand_in(public_port, ctx.clone())).await.unwrap();
        assert_eq!(url.path(), "/final");
        assert_eq!(res.text().await.unwrap(), "pagina finale");
    }

    #[tokio::test]
    async fn redirect_chains_are_bounded_and_respect_the_allowlist() {
        let (public_port, hits) = serve(|p| {
            let n: usize = p.trim_start_matches("/hop/").parse().unwrap_or(0);
            redirect(&format!("/hop/{}", n + 1))
        })
        .await;
        let ctx = ToolContext::new("r", std::env::temp_dir());
        let r = fetch_following(&format!("http://127.0.0.1:{public_port}/hop/0"), stand_in(public_port, ctx.clone())).await;
        assert!(matches!(r, Err(Error::Runtime(ref m)) if m.contains("troppi reindirizzamenti")), "{:?}", r.map(|(_, u)| u));
        assert_eq!(hits.load(Ordering::SeqCst), MAX_REDIRECTS + 1);

        // With an allowlist, a redirect off it is refused even though the first URL was fine.
        let (p2, _) = serve(|_| redirect("http://elsewhere.example/")).await;
        let mut lan = ToolContext::new("r", std::env::temp_dir());
        lan.allow_private_network = true;
        lan.allow_domains = vec!["127.0.0.1".into()];
        let r = fetch_following(&format!("http://127.0.0.1:{p2}/"), |u| {
            let lan = lan.clone();
            async move { vet_url(&u, &lan).await }
        })
        .await;
        assert!(matches!(r, Err(Error::Denied(ref m)) if m.contains("domini consentiti")), "{:?}", r.map(|(_, u)| u));
    }

    #[tokio::test]
    async fn web_fetch_follows_redirects_through_the_tool() {
        let (port, _) = serve(|p| if p == "/" { redirect("/pagina") } else { ok("contenuto") }).await;
        let mut ctx = ToolContext::new("r", std::env::temp_dir());
        ctx.allow_private_network = true; // the stand-in server is on loopback
        let out = WebFetchTool.call(json!({"url": format!("http://127.0.0.1:{port}/")}), &ctx).await.unwrap();
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains(&format!("URL: http://127.0.0.1:{port}/pagina")) && out.content.contains("contenuto"),
            "{}",
            out.content
        );
        // Without that permission the same URL is refused before any request is made.
        let strict = ToolContext::new("r", std::env::temp_dir());
        assert!(WebFetchTool.call(json!({"url": format!("http://127.0.0.1:{port}/")}), &strict).await.is_err());
    }
}
