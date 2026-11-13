//! Access tokens of the HTTP API: one per paired device, one per app of the person, and the owner
//! token of this computer's own UI and CLI.
//!
//! * A token is `aiw_` + 256 random bits (base64url). The database keeps only a salted SHA-256 of
//!   it, the first characters (to recognise it in a list) and its dates: the secret is shown once,
//!   when it is created. The owner token is the exception that cannot be avoided: the `token` file in
//!   the data folder (owner-only permissions) holds it so that the CLI and the desktop shell on this
//!   computer can read it; the database holds only its hash too.
//! * What a token may do is its list of scopes (see `aiw-server`'s `scopes` module for the table of
//!   routes): `full` for the owner, `device` for a paired phone or computer, `gateway` for a third
//!   party app that only speaks the OpenAI/Anthropic-compatible `/v1`, optionally with `a2a` and `mcp`.
//! * A token can be revoked (it stops working at once and the streams it opened are closed), rotated
//!   (a new secret replaces the old one, which stops working) and expires on its date if it has one.
//! * Bad tokens are rate limited per client address, like the pairing code.

use crate::pairing::constant_time_eq;
use crate::store::TokenRow;
use crate::Engine;
use aiw_core::api::{ApiTokenCreated, ApiTokenInfo};
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// Id of the row that stands for the owner token.
pub const OWNER_ID: &str = "tok_owner";
/// Every token created by this module starts like this.
pub const PREFIX: &str = "aiw_";
/// Characters of the token kept in clear to recognise it (and to look it up).
const VISIBLE: usize = 12;
/// A token seen again is written to the database at most this often.
const TOUCH_EVERY: Duration = Duration::from_secs(30);
/// Rejections of the same revoked or expired token are logged at most this often.
const AUDIT_EVERY: Duration = Duration::from_secs(60);
/// Longest name of a token or of a paired device.
pub const MAX_NAME_CHARS: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Owner,
    Device,
    Gateway,
}

impl TokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TokenKind::Owner => "owner",
            TokenKind::Device => "device",
            TokenKind::Gateway => "gateway",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(TokenKind::Owner),
            "device" => Some(TokenKind::Device),
            "gateway" => Some(TokenKind::Gateway),
            _ => None,
        }
    }
}

/// What a token may reach. `Full` covers everything, `Device` also covers the three surfaces of
/// third-party apps (`Gateway`, `A2a`, `Mcp`: a paired computer uses them all).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Full,
    Device,
    Gateway,
    A2a,
    Mcp,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Full => "full",
            Scope::Device => "device",
            Scope::Gateway => "gateway",
            Scope::A2a => "a2a",
            Scope::Mcp => "mcp",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "full" => Some(Scope::Full),
            "device" => Some(Scope::Device),
            "gateway" => Some(Scope::Gateway),
            "a2a" => Some(Scope::A2a),
            "mcp" => Some(Scope::Mcp),
            _ => None,
        }
    }
    /// Does holding `self` allow what `needed` allows?
    pub fn covers(self, needed: Scope) -> bool {
        self == needed || self == Scope::Full || (self == Scope::Device && matches!(needed, Scope::Gateway | Scope::A2a | Scope::Mcp))
    }
}

/// A token that was recognised and is in force.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenAuth {
    pub id: String,
    pub name: String,
    pub kind: TokenKind,
    pub scopes: Vec<Scope>,
    pub device_id: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenCheck {
    Valid(TokenAuth),
    /// A token that existed and was revoked.
    Revoked {
        id: String,
        name: String,
    },
    /// A token that existed and whose date has passed.
    Expired {
        id: String,
        name: String,
    },
    /// Not a token of this engine.
    Unknown,
}

// ------------------------------------------------------------------ limiter

/// Failed tokens tolerated per [`TokenGuard::WINDOW`] from one address.
pub const MAX_BAD_TOKENS: usize = 10;
const MAX_TRACKED: usize = 512;

/// Sliding-window limiter of the bad tokens of each client address (the same approach as the
/// pairing code's [`crate::pairing::PairGuard`]): after [`MAX_BAD_TOKENS`] failures an address is
/// refused for [`TokenGuard::LOCKOUT`], and a right token from it is refused too until the wait is
/// over, so guessing in bulk does not pay.
#[derive(Default)]
pub(crate) struct TokenGuard {
    clients: HashMap<String, Client>,
}

#[derive(Default)]
struct Client {
    failures: VecDeque<Instant>,
    locked_until: Option<Instant>,
}

impl TokenGuard {
    pub const WINDOW: Duration = Duration::from_secs(600);
    pub const LOCKOUT: Duration = Duration::from_secs(300);

    /// `Some(wait)` while `client` is refused.
    pub fn check(&mut self, client: &str, now: Instant) -> Option<Duration> {
        let c = self.clients.get_mut(client)?;
        match c.locked_until {
            Some(until) if until > now => Some(until - now),
            Some(_) => {
                c.locked_until = None;
                None
            }
            None => None,
        }
    }

    /// Records a bad token. `true` when this failure tripped the limit.
    pub fn record_failure(&mut self, client: &str, now: Instant) -> bool {
        if self.clients.len() >= MAX_TRACKED && !self.clients.contains_key(client) {
            self.clients.retain(|_, c| {
                while c.failures.front().is_some_and(|t| now.duration_since(*t) >= Self::WINDOW) {
                    c.failures.pop_front();
                }
                !c.failures.is_empty() || c.locked_until.is_some_and(|u| u > now)
            });
            if self.clients.len() >= MAX_TRACKED {
                return false;
            }
        }
        let c = self.clients.entry(client.to_string()).or_default();
        while c.failures.front().is_some_and(|t| now.duration_since(*t) >= Self::WINDOW) {
            c.failures.pop_front();
        }
        c.failures.push_back(now);
        if c.failures.len() >= MAX_BAD_TOKENS {
            c.locked_until = Some(now + Self::LOCKOUT);
            c.failures.clear();
            return true;
        }
        false
    }
}

/// Runtime state of the token system (all in memory).
pub(crate) struct TokenRuntime {
    guard: Mutex<TokenGuard>,
    touched: Mutex<HashMap<String, Instant>>,
    audited: Mutex<HashMap<String, Instant>>,
    revoked: broadcast::Sender<String>,
}

impl Default for TokenRuntime {
    fn default() -> Self {
        Self { guard: Default::default(), touched: Default::default(), audited: Default::default(), revoked: broadcast::channel(256).0 }
    }
}

// ------------------------------------------------------------------ secrets

/// A new secret: `aiw_` and 256 random bits.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).expect("the system random generator");
    format!("{PREFIX}{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn new_salt() -> String {
    let mut bytes = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).expect("the system random generator");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn hash_token(salt: &str, token: &str) -> String {
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update([0u8]);
    h.update(token.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn visible_prefix(token: &str) -> String {
    token.chars().take(VISIBLE).collect()
}

fn parse_time(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&chrono::Utc))
}

fn is_expired(expires_at: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> bool {
    expires_at.and_then(parse_time).is_some_and(|t| t <= now)
}

fn info(r: &TokenRow) -> ApiTokenInfo {
    ApiTokenInfo {
        id: r.id.clone(),
        name: r.name.clone(),
        kind: r.kind.clone(),
        device_id: r.device_id.clone(),
        prefix: format!("{}…", r.prefix),
        scopes: r.scopes.clone(),
        created_at: r.created_at.clone(),
        last_used_at: r.last_used_at.clone(),
        last_used_ip: r.last_used_ip.clone(),
        expires_at: r.expires_at.clone(),
        revoked_at: r.revoked_at.clone(),
    }
}

/// A printable, bounded name (no control characters).
fn clean_name(s: &str, fallback: &str) -> String {
    let n: String = s.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_NAME_CHARS).collect();
    if n.is_empty() {
        fallback.to_string()
    } else {
        n
    }
}

/// A device id as sent by the device itself: letters, digits and `-_.:`, bounded.
fn clean_device_id(s: &str) -> Option<String> {
    let id: String = s.trim().chars().take(64).collect();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))).then_some(id)
}

impl Engine {
    // ------------------------------------------------------------------ owner

    /// Makes sure the owner token has its row (first start after the upgrade: the single token that
    /// always existed becomes the `owner` token) and retires rows of owner tokens that are not the
    /// current one (the `token` file was replaced).
    pub(crate) fn sync_owner_token(&self) -> Result<()> {
        let owner = self.0.token.as_str();
        let rows = self.0.store.token_list()?;
        let mut present = false;
        for r in rows.iter().filter(|r| r.kind == "owner" && r.revoked_at.is_none()) {
            if constant_time_eq(&hash_token(&r.salt, owner), &r.hash) {
                present = true;
            } else {
                self.0.store.token_revoke(&r.id, &now_rfc3339())?;
            }
        }
        if !present {
            let salt = new_salt();
            // The id of the first owner row is stable; a replaced token gets a new row.
            let id = if rows.iter().any(|r| r.id == OWNER_ID) { new_id("tok") } else { OWNER_ID.to_string() };
            self.0.store.token_insert(&TokenRow {
                id,
                name: "Questo computer (app e riga di comando)".into(),
                kind: "owner".into(),
                device_id: None,
                hash: hash_token(&salt, owner),
                salt,
                prefix: visible_prefix(owner),
                scopes: vec!["full".into()],
                created_at: now_rfc3339(),
                last_used_at: None,
                last_used_ip: None,
                expires_at: None,
                revoked_at: None,
            })?;
        }
        Ok(())
    }

    fn owner_row_id(&self) -> Option<String> {
        self.0.store.token_list().ok()?.into_iter().find(|r| r.kind == "owner" && r.revoked_at.is_none()).map(|r| r.id)
    }

    // ------------------------------------------------------------------ check

    /// Recognises a presented token. The owner token is compared in constant time; every other one
    /// is looked up by its visible prefix and its salted hash compared in constant time.
    pub fn authenticate_token(&self, token: &str, ip: Option<&str>) -> TokenCheck {
        if token.is_empty() || token.len() > 256 {
            return TokenCheck::Unknown;
        }
        if constant_time_eq(token, &self.0.token) {
            self.touch(OWNER_ID, ip, || self.owner_row_id());
            return TokenCheck::Valid(TokenAuth {
                id: OWNER_ID.into(),
                name: "Questo computer".into(),
                kind: TokenKind::Owner,
                scopes: vec![Scope::Full],
                device_id: None,
                expires_at: None,
            });
        }
        if !token.starts_with(PREFIX) {
            return TokenCheck::Unknown;
        }
        let Ok(rows) = self.0.store.token_by_prefix(&visible_prefix(token)) else { return TokenCheck::Unknown };
        for r in rows.into_iter().filter(|r| r.kind != "owner") {
            if !constant_time_eq(&hash_token(&r.salt, token), &r.hash) {
                continue;
            }
            if r.revoked_at.is_some() {
                return TokenCheck::Revoked { id: r.id, name: r.name };
            }
            if is_expired(r.expires_at.as_deref(), chrono::Utc::now()) {
                return TokenCheck::Expired { id: r.id, name: r.name };
            }
            self.touch(&r.id, ip, || None);
            let kind = TokenKind::parse(&r.kind).unwrap_or(TokenKind::Device);
            return TokenCheck::Valid(TokenAuth {
                scopes: r.scopes.iter().filter_map(|s| Scope::parse(s)).collect(),
                id: r.id,
                name: r.name,
                kind,
                device_id: r.device_id,
                expires_at: r.expires_at,
            });
        }
        TokenCheck::Unknown
    }

    fn touch(&self, id: &str, ip: Option<&str>, resolve: impl FnOnce() -> Option<String>) {
        let now = Instant::now();
        {
            let mut t = self.0.tokens.touched.lock().unwrap();
            if t.get(id).is_some_and(|last| now.duration_since(*last) < TOUCH_EVERY) {
                return;
            }
            if t.len() > 1024 {
                t.retain(|_, last| now.duration_since(*last) < TOUCH_EVERY);
            }
            t.insert(id.to_string(), now);
        }
        let row_id = if id == OWNER_ID { resolve() } else { Some(id.to_string()) };
        if let Some(row_id) = row_id {
            let _ = self.0.store.token_touch(&row_id, &now_rfc3339(), ip);
        }
    }

    // ------------------------------------------------------------------ limiter and audit

    /// `Some(wait)` while this client address is refused because of too many bad tokens.
    pub fn token_lockout(&self, ip: &str) -> Option<Duration> {
        self.0.tokens.guard.lock().unwrap().check(ip, Instant::now())
    }

    /// Records a bad token from `ip`; the lockout is written to the audit log.
    pub fn token_failure(&self, ip: &str) {
        let tripped = self.0.tokens.guard.lock().unwrap().record_failure(ip, Instant::now());
        if tripped {
            self.audit_system(
                "token_lockout",
                &format!("{MAX_BAD_TOKENS} token non validi da {ip}: accessi da quell'indirizzo sospesi per qualche minuto"),
            );
        }
    }

    /// Logs the use of a revoked or expired token (once a minute for each token).
    pub fn audit_token_rejected(&self, id: &str, name: &str, why: &str, ip: Option<&str>) {
        let now = Instant::now();
        {
            let mut a = self.0.tokens.audited.lock().unwrap();
            if a.get(id).is_some_and(|last| now.duration_since(*last) < AUDIT_EVERY) {
                return;
            }
            if a.len() > 1024 {
                a.retain(|_, last| now.duration_since(*last) < AUDIT_EVERY);
            }
            a.insert(id.to_string(), now);
        }
        self.audit_system("token_rejected", &format!("token «{name}» ({id}) {why}{}", ip.map(|i| format!(", da {i}")).unwrap_or_default()));
    }

    /// Ids of tokens that are revoked or rotated, as they happen: whoever holds a stream opened with
    /// that token closes it.
    pub fn token_revocations(&self) -> broadcast::Receiver<String> {
        self.0.tokens.revoked.subscribe()
    }

    /// Has this token been revoked (or does it no longer exist)? Used by a stream that lagged behind
    /// the revocation notices.
    pub fn token_is_active(&self, id: &str) -> bool {
        if id == OWNER_ID {
            return true;
        }
        self.0
            .store
            .token_get(id)
            .ok()
            .flatten()
            .is_some_and(|r| r.revoked_at.is_none() && !is_expired(r.expires_at.as_deref(), chrono::Utc::now()))
    }

    // ------------------------------------------------------------------ management

    /// The tokens, newest first (revoked ones included, with their date).
    pub fn api_tokens(&self) -> Vec<ApiTokenInfo> {
        self.0.store.token_list().unwrap_or_default().iter().map(info).collect()
    }

    /// Creates a token. `kind` is `device` or `gateway`; the secret is in the result and nowhere else.
    pub fn create_api_token(
        &self,
        name: &str,
        kind: &str,
        device_id: Option<&str>,
        expires_in_days: Option<u32>,
        extra_scopes: Option<&[String]>,
    ) -> Result<ApiTokenCreated> {
        let kind = TokenKind::parse(kind)
            .filter(|k| *k != TokenKind::Owner)
            .ok_or_else(|| Error::Invalid("tipo di token non valido: usa «gateway» (app di terzi) o «device» (dispositivo)".into()))?;
        let name = clean_name(name, "");
        if name.is_empty() {
            return Err(Error::Invalid("dai un nome al token (per esempio l'app che lo userà)".into()));
        }
        let scopes: Vec<String> = match kind {
            TokenKind::Gateway => {
                let mut s = vec!["gateway".to_string()];
                for extra in extra_scopes.unwrap_or_default() {
                    match Scope::parse(extra) {
                        Some(Scope::A2a) | Some(Scope::Mcp) => {
                            if !s.contains(extra) {
                                s.push(extra.clone());
                            }
                        }
                        _ => return Err(Error::Invalid(format!("ambito non valido per un token di app: {extra} (solo a2a e mcp)"))),
                    }
                }
                s
            }
            _ => {
                if extra_scopes.is_some_and(|e| !e.is_empty()) {
                    return Err(Error::Invalid("un token di dispositivo ha sempre lo stesso ambito: non se ne scelgono altri".into()));
                }
                vec!["device".into()]
            }
        };
        if expires_in_days == Some(0) || expires_in_days.is_some_and(|d| d > 3650) {
            return Err(Error::Invalid("la scadenza va da 1 giorno a 10 anni".into()));
        }
        let expires_at = expires_in_days
            .map(|d| (chrono::Utc::now() + chrono::Duration::days(d as i64)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        let device_id = match (kind, device_id) {
            (TokenKind::Device, Some(d)) => {
                Some(clean_device_id(d).ok_or_else(|| Error::Invalid("identificativo del dispositivo non valido".into()))?)
            }
            (TokenKind::Device, None) => Some(new_id("dev")),
            _ => None,
        };
        let created = self.insert_token(&name, kind, device_id, expires_at, scopes)?;
        self.audit_system(
            "token_create",
            &format!(
                "creato il token «{}» ({}, {}{})",
                created.info.name,
                created.info.kind,
                created.info.id,
                created.info.expires_at.as_deref().map(|e| format!(", scade {e}")).unwrap_or_default()
            ),
        );
        Ok(created)
    }

    fn insert_token(
        &self,
        name: &str,
        kind: TokenKind,
        device_id: Option<String>,
        expires_at: Option<String>,
        scopes: Vec<String>,
    ) -> Result<ApiTokenCreated> {
        let token = generate_token();
        let salt = new_salt();
        let row = TokenRow {
            id: new_id("tok"),
            name: name.to_string(),
            kind: kind.as_str().into(),
            device_id,
            hash: hash_token(&salt, &token),
            salt,
            prefix: visible_prefix(&token),
            scopes,
            created_at: now_rfc3339(),
            last_used_at: None,
            last_used_ip: None,
            expires_at,
            revoked_at: None,
        };
        self.0.store.token_insert(&row)?;
        Ok(ApiTokenCreated { info: info(&row), token })
    }

    /// Issues the token of a device that has just paired. The device keeps its id (when it sent a valid
    /// one) and a new pairing of the same device replaces its old token: the old one stops working.
    pub fn issue_device_token(&self, device_id: Option<&str>, name: Option<&str>) -> Result<ApiTokenCreated> {
        let device_id = device_id.and_then(clean_device_id);
        if let Some(d) = &device_id {
            let n = self.revoke_device_tokens(d)?;
            if n > 0 {
                self.audit_system("token_replace", &format!("il dispositivo {d} si è accoppiato di nuovo: {n} token precedenti revocati"));
            }
        }
        let fallback = format!("Dispositivo accoppiato il {}", chrono::Local::now().format("%d/%m/%Y %H:%M"));
        let name = clean_name(name.unwrap_or_default(), &fallback);
        self.create_api_token(&name, "device", device_id.as_deref(), None, None)
    }

    /// Revokes every active token of a device; returns how many.
    pub fn revoke_device_tokens(&self, device_id: &str) -> Result<usize> {
        let mut n = 0;
        for r in self.0.store.token_list()?.into_iter().filter(|r| r.device_id.as_deref() == Some(device_id) && r.revoked_at.is_none()) {
            if self.0.store.token_revoke(&r.id, &now_rfc3339())? {
                let _ = self.0.tokens.revoked.send(r.id);
                n += 1;
            }
        }
        Ok(n)
    }

    /// Sets or clears the expiry of a token (an RFC 3339 date). Not offered by the API: for the tests and for tooling.
    pub fn set_api_token_expiry(&self, id: &str, expires_at: Option<&str>) -> Result<()> {
        self.0.store.token_get(id)?.ok_or_else(|| Error::NotFound(format!("token {id} non trovato")))?;
        self.0.store.token_set_expiry(id, expires_at)
    }

    /// The tokens as stored (hash and salt included), as one text: for the test that checks that no secret is at rest.
    pub fn api_tokens_at_rest(&self) -> String {
        self.0.store.token_dump()
    }

    /// Revokes a token at once: it stops working and the streams opened with it are closed.
    pub fn revoke_api_token(&self, id: &str) -> Result<()> {
        let r = self.0.store.token_get(id)?.ok_or_else(|| Error::NotFound(format!("token {id} non trovato")))?;
        if r.kind == "owner" {
            return Err(Error::Denied(
                "il token di questo computer non si revoca dall'API: per cambiarlo ferma il servizio, cancella il file «token» nella cartella dei dati e riavvia"
                    .into(),
            ));
        }
        if self.0.store.token_revoke(id, &now_rfc3339())? {
            let _ = self.0.tokens.revoked.send(id.to_string());
            self.audit_system("token_revoke", &format!("revocato il token «{}» ({}, {})", r.name, r.kind, r.id));
        }
        Ok(())
    }

    /// Replaces a token with a new one (same name, kind, scopes and expiry): the old secret stops
    /// working at once.
    pub fn rotate_api_token(&self, id: &str) -> Result<ApiTokenCreated> {
        let r = self.0.store.token_get(id)?.ok_or_else(|| Error::NotFound(format!("token {id} non trovato")))?;
        if r.kind == "owner" {
            return Err(Error::Denied(
                "il token di questo computer non si ruota dall'API: per cambiarlo ferma il servizio, cancella il file «token» nella cartella dei dati e riavvia"
                    .into(),
            ));
        }
        if r.revoked_at.is_some() {
            return Err(Error::Conflict("il token è già stato revocato: creane uno nuovo".into()));
        }
        let kind = TokenKind::parse(&r.kind).unwrap_or(TokenKind::Gateway);
        let created = self.insert_token(&r.name, kind, r.device_id.clone(), r.expires_at.clone(), r.scopes.clone())?;
        self.0.store.token_revoke(&r.id, &now_rfc3339())?;
        let _ = self.0.tokens.revoked.send(r.id.clone());
        self.audit_system("token_rotate", &format!("ruotato il token «{}»: {} sostituisce {}", r.name, created.info.id, r.id));
        Ok(created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_long_random_and_prefixed() {
        let a = generate_token();
        let b = generate_token();
        assert!(a.starts_with(PREFIX) && a.len() >= 4 + 43, "{a}");
        assert_ne!(a, b);
        // 256 bits survive the encoding.
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&a[4..]).unwrap();
        assert_eq!(raw.len(), 32);
    }

    #[test]
    fn hashes_depend_on_salt_and_token() {
        let h = hash_token("s1", "aiw_x");
        assert_eq!(h.len(), 64);
        assert_ne!(h, hash_token("s2", "aiw_x"));
        assert_ne!(h, hash_token("s1", "aiw_y"));
        assert_eq!(h, hash_token("s1", "aiw_x"));
    }

    #[test]
    fn scopes_cover_what_they_should() {
        use Scope::*;
        assert!(Full.covers(Device) && Full.covers(Gateway) && Full.covers(Full));
        assert!(Device.covers(Gateway) && Device.covers(A2a) && Device.covers(Mcp) && Device.covers(Device));
        assert!(!Device.covers(Full));
        assert!(Gateway.covers(Gateway) && !Gateway.covers(Device) && !Gateway.covers(A2a) && !Gateway.covers(Full));
        assert!(A2a.covers(A2a) && !A2a.covers(Mcp));
    }

    #[test]
    fn the_guard_locks_one_address_and_forgets_after_the_window() {
        let mut g = TokenGuard::default();
        let t0 = Instant::now();
        for i in 0..MAX_BAD_TOKENS - 1 {
            assert!(!g.record_failure("10.0.0.1", t0), "{i}");
        }
        assert_eq!(g.check("10.0.0.1", t0), None);
        assert!(g.record_failure("10.0.0.1", t0));
        assert_eq!(g.check("10.0.0.1", t0), Some(TokenGuard::LOCKOUT));
        assert_eq!(g.check("10.0.0.2", t0), None, "others are not affected");
        assert_eq!(g.check("10.0.0.1", t0 + TokenGuard::LOCKOUT), None);
        // Failures age out of the window.
        for _ in 0..MAX_BAD_TOKENS - 1 {
            g.record_failure("10.0.0.3", t0);
        }
        assert!(!g.record_failure("10.0.0.3", t0 + TokenGuard::WINDOW));
    }

    #[test]
    fn the_guard_table_is_bounded() {
        let mut g = TokenGuard::default();
        let t0 = Instant::now();
        for i in 0..MAX_TRACKED * 2 {
            g.record_failure(&format!("c{i}"), t0);
        }
        assert!(g.clients.len() <= MAX_TRACKED);
    }

    #[test]
    fn names_and_ids_are_cleaned() {
        assert_eq!(clean_name("  App\u{7}  ", "x"), "App");
        assert_eq!(clean_name("   ", "x"), "x");
        assert_eq!(clean_name(&"a".repeat(200), "x").chars().count(), MAX_NAME_CHARS);
        assert_eq!(clean_device_id("pc-1.local").as_deref(), Some("pc-1.local"));
        assert_eq!(clean_device_id("a b"), None);
        assert_eq!(clean_device_id("../x"), None);
    }
}
