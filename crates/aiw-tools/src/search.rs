//! `web_search`: searches the web through the provider the person chose in the settings.
//!
//! Providers (`search.provider`): `searxng` (an instance the person runs, possibly on this computer or
//! the LAN), `brave` (Brave Search API, needs a key), `duckduckgo` (no key, parses the HTML results
//! page), `auto` (SearXNG when its address is set, else Brave when a key is set, else DuckDuckGo) and
//! `off` (the tool is not offered). The provider, the address and the key come from the settings, never
//! from the model: the model can only say what to search for and how many results it wants.

use crate::{Risk, Tool, ToolContext, ToolOutput};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Time budget of one search.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);
/// Upper bound for a provider answer.
const MAX_BODY_BYTES: usize = 2 << 20;
/// Characters kept of each snippet.
const SNIPPET_CHARS: usize = 300;
const DEFAULT_RESULTS: i64 = 5;
const MAX_RESULTS: i64 = 10;

const BRAVE_ENDPOINT: &str = "https://api.search.brave.com/res/v1/web/search";
const DUCKDUCKGO_ENDPOINT: &str = "https://html.duckduckgo.com/html/";

/// What the settings say about the search.
#[derive(Clone, Default, PartialEq)]
pub struct Config {
    /// `auto` (also when empty), `off`, `searxng`, `brave` or `duckduckgo`.
    pub provider: String,
    pub searxng_url: String,
    pub brave_api_key: String,
}

// The key never ends up in a log through `{:?}`.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("provider", &self.provider)
            .field("searxng_url", &self.searxng_url)
            .field("brave_api_key", &if self.brave_api_key.is_empty() { "" } else { "<set>" })
            .finish()
    }
}

/// The search settings shared by the tool and the engine (the engine refreshes them when the
/// settings change).
#[derive(Default)]
pub struct SearchSettings {
    cfg: RwLock<Config>,
}

pub type Shared = Arc<SearchSettings>;

impl SearchSettings {
    pub fn new(cfg: Config) -> Shared {
        Arc::new(Self { cfg: RwLock::new(cfg) })
    }
    pub fn set(&self, cfg: Config) {
        *self.cfg.write().unwrap() = cfg;
    }
    pub fn get(&self) -> Config {
        self.cfg.read().unwrap().clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Searxng,
    Brave,
    DuckDuckGo,
}

impl Provider {
    fn label(self) -> &'static str {
        match self {
            Provider::Searxng => "SearXNG",
            Provider::Brave => "Brave Search",
            Provider::DuckDuckGo => "DuckDuckGo",
        }
    }
}

/// The provider the settings select; `None` when the search is off.
pub fn resolve(cfg: &Config) -> Option<Provider> {
    match cfg.provider.trim() {
        "off" => None,
        "searxng" => Some(Provider::Searxng),
        "brave" => Some(Provider::Brave),
        "duckduckgo" => Some(Provider::DuckDuckGo),
        // `auto`, empty or unknown values.
        _ if !cfg.searxng_url.trim().is_empty() => Some(Provider::Searxng),
        _ if !cfg.brave_api_key.trim().is_empty() => Some(Provider::Brave),
        _ => Some(Provider::DuckDuckGo),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub struct WebSearchTool {
    settings: Shared,
    /// Endpoints of the providers with a fixed address; fields (not settings) so tests can point them
    /// at a local server.
    brave_endpoint: String,
    duckduckgo_endpoint: String,
}

impl WebSearchTool {
    pub fn new(settings: Shared) -> Self {
        Self::with_endpoints(settings, BRAVE_ENDPOINT, DUCKDUCKGO_ENDPOINT)
    }
    pub fn with_endpoints(settings: Shared, brave: &str, duckduckgo: &str) -> Self {
        Self { settings, brave_endpoint: brave.into(), duckduckgo_endpoint: duckduckgo.into() }
    }

    fn client(&self, follow_redirects: bool) -> Result<reqwest::Client> {
        let policy = if follow_redirects { reqwest::redirect::Policy::limited(3) } else { reqwest::redirect::Policy::none() };
        reqwest::Client::builder()
            .timeout(SEARCH_TIMEOUT)
            .redirect(policy)
            .user_agent("Mozilla/5.0 (compatible; AIWorkspaceLocal/0.1; +https://github.com/davides93/ai-workspace-local)")
            .build()
            .map_err(Error::other)
    }

    /// Looks the query up; `Err(message)` is shown to the model as a tool error.
    async fn search(
        &self,
        cfg: &Config,
        provider: Provider,
        query: &str,
        n: usize,
        ctx: &ToolContext,
    ) -> std::result::Result<Vec<Hit>, String> {
        match provider {
            Provider::Searxng => self.searxng(cfg, query, n).await,
            Provider::Brave => self.brave(cfg, query, n, ctx).await,
            Provider::DuckDuckGo => self.duckduckgo(query, n, ctx).await,
        }
    }

    async fn searxng(&self, cfg: &Config, query: &str, n: usize) -> std::result::Result<Vec<Hit>, String> {
        let base = cfg.searxng_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return Err("SearXNG non è configurato: indica l'indirizzo dell'istanza in Impostazioni, «Ricerca web».".into());
        }
        let mut url = reqwest::Url::parse(&format!("{base}/search")).map_err(|e| format!("L'indirizzo di SearXNG non è valido: {e}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("L'indirizzo di SearXNG deve iniziare con http:// o https://.".into());
        }
        url.query_pairs_mut().append_pair("q", query).append_pair("format", "json");
        let client = self.client(true).map_err(|e| e.to_string())?;
        let res = client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("SearXNG non è raggiungibile ({base}): {}", short_error(e)))?;
        let status = res.status();
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err("SearXNG ha rifiutato la richiesta (403): verifica che l'istanza consenta il formato json (search.formats in settings.yml).".into());
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err("SearXNG risponde con troppe richieste (429): riprova tra poco.".into());
        }
        if !status.is_success() {
            return Err(format!("SearXNG ha risposto con errore {status}."));
        }
        let (body, _) = crate::web::read_capped(res, MAX_BODY_BYTES).await.map_err(|e| e.to_string())?;
        parse_searxng(&body, n)
    }

    async fn brave(&self, cfg: &Config, query: &str, n: usize, ctx: &ToolContext) -> std::result::Result<Vec<Hit>, String> {
        let key = cfg.brave_api_key.trim();
        if key.is_empty() {
            return Err(
                "Manca la chiave di Brave Search: inseriscila in Impostazioni, «Ricerca web», oppure scegli un altro motore.".into()
            );
        }
        check_allowed(&self.brave_endpoint, ctx)?;
        let mut url = reqwest::Url::parse(&self.brave_endpoint).map_err(|e| format!("Indirizzo di Brave Search non valido: {e}"))?;
        url.query_pairs_mut().append_pair("q", query).append_pair("count", &n.to_string());
        // No redirects: the key travels in a header and must not follow a request to another host.
        let client = self.client(false).map_err(|e| e.to_string())?;
        let res = client
            .get(url)
            .header("X-Subscription-Token", key)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("Brave Search non è raggiungibile: {}", short_error(e)))?;
        let status = res.status();
        match status.as_u16() {
            401 | 403 | 422 => {
                return Err(format!("Brave Search ha rifiutato la chiave ({status}): controllala in Impostazioni, «Ricerca web»."))
            }
            429 => {
                return Err(
                    "Brave Search ha raggiunto il limite di richieste (429): riprova più tardi o controlla il piano della tua chiave."
                        .into(),
                )
            }
            _ if !status.is_success() => return Err(format!("Brave Search ha risposto con errore {status}.")),
            _ => {}
        }
        let (body, _) = crate::web::read_capped(res, MAX_BODY_BYTES).await.map_err(|e| e.to_string())?;
        parse_brave(&body, n)
    }

    async fn duckduckgo(&self, query: &str, n: usize, ctx: &ToolContext) -> std::result::Result<Vec<Hit>, String> {
        check_allowed(&self.duckduckgo_endpoint, ctx)?;
        let client = self.client(false).map_err(|e| e.to_string())?;
        let res = client
            .post(&self.duckduckgo_endpoint)
            .form(&[("q", query)])
            .send()
            .await
            .map_err(|e| format!("DuckDuckGo non è raggiungibile: {}", short_error(e)))?;
        let status = res.status();
        let (body, _) = crate::web::read_capped(res, MAX_BODY_BYTES).await.map_err(|e| e.to_string())?;
        let hits = parse_duckduckgo(&body, n);
        if hits.is_empty() && (status.as_u16() == 202 || status == reqwest::StatusCode::TOO_MANY_REQUESTS || looks_blocked(&body)) {
            return Err("DuckDuckGo ha bloccato la richiesta automatica (verifica anti-bot): riprova più tardi, oppure usa SearXNG o Brave Search in Impostazioni, «Ricerca web».".into());
        }
        if !status.is_success() && hits.is_empty() {
            return Err(format!("DuckDuckGo ha risposto con errore {status}."));
        }
        Ok(hits)
    }
}

/// With a domain allowlist (`network.allow_domains`) the fixed-address providers must be on it.
fn check_allowed(endpoint: &str, ctx: &ToolContext) -> std::result::Result<(), String> {
    if ctx.allow_domains.is_empty() {
        return Ok(());
    }
    let host = reqwest::Url::parse(endpoint).ok().and_then(|u| u.host_str().map(str::to_lowercase)).unwrap_or_default();
    if ctx.allow_domains.iter().any(|d| host == *d || host.ends_with(&format!(".{d}"))) {
        Ok(())
    } else {
        Err(format!("{host} non è tra i domini consentiti: aggiungilo a network.allow_domains oppure usa SearXNG."))
    }
}

/// A transport error without the URL (it carries the query text).
fn short_error(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "tempo scaduto".into()
    } else if e.is_connect() {
        "connessione rifiutata o host sconosciuto".into()
    } else {
        e.without_url().to_string()
    }
}

fn looks_blocked(body: &str) -> bool {
    let l = body.to_lowercase();
    l.contains("anomaly") || l.contains("captcha") || l.contains("unusual traffic")
}

// ---------------------------------------------------------------- parsing

fn parse_searxng(body: &str, n: usize) -> std::result::Result<Vec<Hit>, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| {
        "SearXNG non ha risposto in JSON: verifica che l'indirizzo sia quello dell'istanza e che il formato json sia abilitato.".to_string()
    })?;
    let results = v.get("results").and_then(|r| r.as_array()).ok_or("La risposta di SearXNG non contiene risultati nel formato atteso.")?;
    Ok(hits_from(results, "title", "url", "content", n))
}

fn parse_brave(body: &str, n: usize) -> std::result::Result<Vec<Hit>, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "La risposta di Brave Search non è JSON valido.".to_string())?;
    // A query without hits has no `web` section at all.
    let results = v.pointer("/web/results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    Ok(hits_from(&results, "title", "url", "description", n))
}

fn hits_from(results: &[Value], title: &str, url: &str, snippet: &str, n: usize) -> Vec<Hit> {
    results
        .iter()
        .filter_map(|r| {
            let u = r.get(url)?.as_str()?.trim();
            if !(u.starts_with("http://") || u.starts_with("https://")) {
                return None;
            }
            let t = clean_text(r.get(title).and_then(|v| v.as_str()).unwrap_or(""));
            Some(Hit {
                title: if t.is_empty() { u.to_string() } else { t },
                url: u.to_string(),
                snippet: clip(&clean_text(r.get(snippet).and_then(|v| v.as_str()).unwrap_or("")), SNIPPET_CHARS),
            })
        })
        .take(n)
        .collect()
}

/// Reads the results page of `html.duckduckgo.com`: each result is a link with class `result__a`
/// (title, and a redirect `//duckduckgo.com/l/?uddg=<target>` as href) followed by an element with
/// class `result__snippet`. Tolerant on purpose: attribute order, extra classes and quoting vary.
pub fn parse_duckduckgo(html: &str, n: usize) -> Vec<Hit> {
    let title_re = Regex::new(r#"(?is)<a\b([^>]*\bclass\s*=\s*["'][^"']*\bresult__a\b[^"']*["'][^>]*)>(.*?)</a>"#).unwrap();
    let snippet_re =
        Regex::new(r#"(?is)<(?:a|div|td|span)\b[^>]*\bclass\s*=\s*["'][^"']*\bresult__snippet\b[^"']*["'][^>]*>(.*?)</(?:a|div|td|span)>"#)
            .unwrap();
    let href_re = Regex::new(r#"(?is)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap();
    let anchors: Vec<_> = title_re.captures_iter(html).collect();
    let mut hits = vec![];
    for (i, c) in anchors.iter().enumerate() {
        let whole = c.get(0).unwrap();
        let Some(href) = href_re.captures(&c[1]).and_then(|h| h.get(1).or_else(|| h.get(2))).map(|m| decode_entities(m.as_str())) else {
            continue;
        };
        let Some(url) = real_url(&href) else { continue };
        let end = anchors.get(i + 1).map_or(html.len(), |next| next.get(0).unwrap().start());
        let snippet = snippet_re.captures(&html[whole.end()..end]).map(|s| clip(&clean_text(&s[1]), SNIPPET_CHARS)).unwrap_or_default();
        let title = clean_text(&c[2]);
        hits.push(Hit { title: if title.is_empty() { url.clone() } else { title }, url, snippet });
        if hits.len() >= n {
            break;
        }
    }
    hits
}

/// The page a DuckDuckGo link points to: the `uddg` parameter of its redirect, or the link itself
/// when it is already direct. Ads (`/y.js`) and anything that is not http(s) give `None`.
fn real_url(href: &str) -> Option<String> {
    let href = href.trim();
    let absolute = if let Some(rest) = href.strip_prefix("//") { format!("https://{rest}") } else { href.to_string() };
    let u = reqwest::Url::parse(&absolute).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let is_ddg = u.host_str().is_some_and(|h| h == "duckduckgo.com" || h.ends_with(".duckduckgo.com"));
    if is_ddg {
        if u.path() == "/y.js" {
            return None;
        }
        let target = u.query_pairs().find(|(k, _)| k == "uddg").map(|(_, v)| v.into_owned())?;
        return (target.starts_with("http://") || target.starts_with("https://")).then_some(target);
    }
    Some(u.to_string())
}

// ---------------------------------------------------------------- text

/// Plain text of an HTML fragment: tags dropped, entities decoded, white space collapsed.
fn clean_text(s: &str) -> String {
    let no_tags = Regex::new(r"(?s)<[^>]*>").unwrap().replace_all(s, "");
    decode_entities(&no_tags).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(s: &str) -> String {
    Regex::new(r"&(#[0-9]+|#[xX][0-9a-fA-F]+|[a-zA-Z]+);")
        .unwrap()
        .replace_all(s, |c: &regex::Captures| {
            let e = &c[1];
            let decoded = if let Some(num) = e.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse().ok(),
                };
                code.and_then(char::from_u32).map(String::from)
            } else {
                match e {
                    "amp" => Some("&"),
                    "lt" => Some("<"),
                    "gt" => Some(">"),
                    "quot" => Some("\""),
                    "apos" => Some("'"),
                    "nbsp" => Some(" "),
                    "hellip" => Some("…"),
                    "ndash" => Some("–"),
                    "mdash" => Some("—"),
                    "laquo" => Some("«"),
                    "raquo" => Some("»"),
                    _ => None,
                }
                .map(String::from)
            };
            decoded.unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

/// The text the model reads.
pub fn format_hits(query: &str, provider: Provider, hits: &[Hit]) -> String {
    if hits.is_empty() {
        return format!("Nessun risultato per «{query}» (motore: {}).", provider.label());
    }
    let mut out = format!("Risultati della ricerca web per «{query}» (motore: {}):\n", provider.label());
    for (i, h) in hits.iter().enumerate() {
        out.push_str(&format!("\n{}. {}\n   {}\n", i + 1, h.title, h.url));
        if !h.snippet.is_empty() {
            out.push_str(&format!("   {}\n", h.snippet));
        }
    }
    out
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Cerca sul web e restituisce un elenco di risultati (titolo, indirizzo, anteprima). Per leggere una pagina trovata usa web_fetch."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "query": {"type": "string", "description": "Cosa cercare"},
            "max_results": {"type": "integer", "description": "Quanti risultati (1-10, predefinito 5)", "minimum": 1, "maximum": 10}
        }, "required": ["query"]})
    }
    fn risk(&self) -> Risk {
        Risk::Network
    }
    fn available(&self) -> std::result::Result<(), String> {
        match resolve(&self.settings.get()) {
            Some(_) => Ok(()),
            None => Err("La ricerca web è spenta: la accendi in Impostazioni, «Ricerca web».".into()),
        }
    }
    fn subject(&self, args: &Value) -> Option<String> {
        args.get("query").and_then(|v| v.as_str()).map(String::from)
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let query = args.get("query").and_then(|v| v.as_str()).map(str::trim).filter(|q| !q.is_empty());
        let query = query.ok_or_else(|| Error::Invalid("argomento `query` mancante".into()))?;
        let n = args
            .get("max_results")
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
            .unwrap_or(DEFAULT_RESULTS)
            .clamp(1, MAX_RESULTS) as usize;
        let cfg = self.settings.get();
        let Some(provider) = resolve(&cfg) else {
            return Err(Error::Unsupported("La ricerca web è spenta: la accendi in Impostazioni, «Ricerca web».".into()));
        };
        let found = tokio::select! {
            r = self.search(&cfg, provider, query, n, ctx) => r,
            _ = ctx.cancel.cancelled() => return Err(Error::Cancelled),
        };
        Ok(match found {
            Ok(hits) => ToolOutput::text(format_hits(query, provider, &hits)),
            Err(message) => ToolOutput::error(message),
        })
    }
}

#[cfg(test)]
mod tests;
