use crate::{arg_str, Risk, Tool, ToolContext, ToolOutput};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::net::IpAddr;
use std::time::Duration;

#[derive(Default)]
pub struct WebFetchTool;

/// Rejects non-HTTP schemes, private/loopback targets (unless allowed) and hosts outside the allowlist.
pub async fn check_url(url: &str, ctx: &ToolContext) -> Result<reqwest::Url> {
    let u = reqwest::Url::parse(url).map_err(|e| Error::Invalid(format!("URL non valido: {e}")))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(Error::Denied("solo URL http/https".into()));
    }
    let host = u.host_str().ok_or_else(|| Error::Invalid("URL senza host".into()))?.to_lowercase();
    if !ctx.allow_domains.is_empty() && !ctx.allow_domains.iter().any(|d| host == *d || host.ends_with(&format!(".{d}"))) {
        return Err(Error::Denied(format!("{host} non è tra i domini consentiti")));
    }
    if !ctx.allow_private_network {
        let port = u.port_or_known_default().unwrap_or(80);
        let addrs: Vec<IpAddr> = match host.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => tokio::net::lookup_host((host.as_str(), port)).await.map(|a| a.map(|s| s.ip()).collect()).unwrap_or_default(),
        };
        if host == "localhost" || addrs.iter().any(is_private) {
            return Err(Error::Denied(format!("{host} è un indirizzo locale o privato: abilita la rete locale per questo agente")));
        }
    }
    Ok(u)
}

fn is_private(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_unspecified()
                || v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64
        }
        IpAddr::V6(v) => {
            v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80
        }
    }
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
        let url = check_url(arg_str(&args, "url")?, ctx).await?;
        let max = args.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(20_000).clamp(500, 200_000) as usize;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent("Mozilla/5.0 (compatible; AIWorkspaceLocal/0.1; +https://github.com/davides93/ai-workspace-local)")
            .build()
            .map_err(Error::other)?;
        let res = tokio::select! {
            r = client.get(url.clone()).send() => r.map_err(|e| Error::Runtime(e.to_string()))?,
            _ = ctx.cancel.cancelled() => return Err(Error::Cancelled),
        };
        let status = res.status();
        let ctype = res.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let body = res.text().await.map_err(|e| Error::Runtime(e.to_string()))?;
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
        Ok(if status.is_success() { ToolOutput::text(out) } else { ToolOutput::error(out) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
