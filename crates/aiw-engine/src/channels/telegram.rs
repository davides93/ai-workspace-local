//! Telegram: the Bot API over HTTPS with long polling (`getUpdates`), no webhook, so nothing has to
//! be reachable from the internet. The offset of the last update is kept in the store, so a restart
//! does not answer the same message twice.
//!
//! The bot token only ever goes to the Telegram base address (`https://api.telegram.org`, or a fake
//! server in the tests); error texts have the address (which contains the token) removed.

use super::{set_status, Engine, Inbound, Target, Transport};
use aiw_core::api::{AttachmentUpload, Channel, MAX_ATTACHMENT_BYTES};
use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(super) const DEFAULT_BASE: &str = "https://api.telegram.org";
/// Characters of one Telegram message are 4096; stay under it.
const LIMIT: usize = 4000;

pub(super) struct Telegram {
    client: reqwest::Client,
    base: String,
    token: String,
}

impl Telegram {
    pub(super) fn new(engine: &Engine, ch: &Channel) -> Self {
        let poll = engine.channel_endpoints().poll_s.unwrap_or(25);
        Self {
            client: reqwest::Client::builder().timeout(Duration::from_secs(poll + 20)).build().unwrap_or_default(),
            base: engine.channel_endpoints().telegram.unwrap_or_else(|| DEFAULT_BASE.into()).trim_end_matches('/').to_string(),
            token: ch.secrets.get("bot_token").cloned().unwrap_or_default(),
        }
    }

    async fn call(&self, method: &str, body: Value) -> Result<Value, String> {
        let url = format!("{}/bot{}/{method}", self.base, self.token);
        let resp = self.client.post(url).json(&body).send().await.map_err(|e| describe(e.without_url()))?;
        let status = resp.status();
        let v: Value = resp.json().await.map_err(|e| format!("risposta non valida da Telegram ({})", e.without_url()))?;
        if v["ok"].as_bool() == Some(true) {
            return Ok(v["result"].clone());
        }
        let why = v["description"].as_str().unwrap_or("errore sconosciuto");
        Err(match status.as_u16() {
            401 | 404 => "Telegram non riconosce il token del bot: ricontrollalo con @BotFather".to_string(),
            429 => "Telegram chiede di rallentare: riprovo tra poco".to_string(),
            _ => format!("Telegram risponde: {why}"),
        })
    }

    async fn download(&self, file_id: &str) -> Result<Vec<u8>, String> {
        let info = self.call("getFile", json!({"file_id": file_id})).await?;
        let path = info["file_path"].as_str().ok_or("Telegram non ha dato il file")?;
        let url = format!("{}/file/bot{}/{path}", self.base, self.token);
        let resp = self.client.get(url).send().await.map_err(|e| describe(e.without_url()))?;
        if !resp.status().is_success() {
            return Err("Telegram non ha dato il file".into());
        }
        let bytes = resp.bytes().await.map_err(|e| describe(e.without_url()))?;
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err("il file è troppo grande".into());
        }
        Ok(bytes.to_vec())
    }
}

fn describe(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "Telegram non risponde in tempo".into()
    } else if e.is_connect() {
        "non riesco a raggiungere Telegram: controlla la connessione".into()
    } else {
        format!("errore di rete verso Telegram: {e}")
    }
}

#[async_trait]
impl Transport for Telegram {
    async fn send(&self, to: &Target, text: &str, ask: Option<&str>) -> Result<(), String> {
        let chat: Value = to.conv.parse::<i64>().map(|n| json!(n)).unwrap_or_else(|_| json!(to.conv));
        let mut body = json!({"chat_id": chat, "text": text, "disable_web_page_preview": true});
        if let Some(id) = ask {
            body["reply_markup"] = json!({"inline_keyboard": [[
                {"text": "Sì", "callback_data": format!("apr:{id}:y")},
                {"text": "No", "callback_data": format!("apr:{id}:n")}
            ]]});
        }
        self.call("sendMessage", body).await.map(|_| ())
    }

    fn limit(&self) -> usize {
        LIMIT
    }
}

pub(super) async fn test(engine: &Engine, ch: &Channel) -> Result<String, String> {
    let t = Telegram::new(engine, ch);
    let me = t.call("getMe", json!({})).await?;
    Ok(format!("Collegato al bot @{}.", me["username"].as_str().unwrap_or("?")))
}

fn display_name(from: &Value) -> String {
    let full = format!("{} {}", from["first_name"].as_str().unwrap_or(""), from["last_name"].as_str().unwrap_or("")).trim().to_string();
    if !full.is_empty() {
        full
    } else {
        from["username"].as_str().map(|u| format!("@{u}")).unwrap_or_else(|| from["id"].to_string())
    }
}

pub(super) async fn run(
    engine: &Engine,
    ch: &Channel,
    status: &Arc<Mutex<aiw_core::api::ChannelStatus>>,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let t = Telegram::new(engine, ch);
    let me = t.call("getMe", json!({})).await?;
    set_status(status, "connected", format!("Collegato al bot @{}.", me["username"].as_str().unwrap_or("?")));
    let tr: Arc<dyn Transport> = Arc::new(Telegram::new(engine, ch));
    let poll = engine.channel_endpoints().poll_s.unwrap_or(25);
    let mut offset = engine.channel_state(&ch.id).tg_offset;
    loop {
        let updates = tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            r = t.call("getUpdates", json!({"offset": offset, "timeout": poll, "allowed_updates": ["message", "callback_query"]})) => r?,
        };
        let list = updates.as_array().cloned().unwrap_or_default();
        for u in list {
            let Some(id) = u["update_id"].as_i64() else { continue };
            handle(engine, ch, &t, &tr, &u).await;
            // Remembered after the message was handed over: a restart does not repeat it.
            offset = id + 1;
            engine.channel_update_state(&ch.id, |s| s.tg_offset = offset);
        }
    }
}

async fn handle(engine: &Engine, ch: &Channel, t: &Telegram, tr: &Arc<dyn Transport>, u: &Value) {
    if let Some(cb) = u.get("callback_query") {
        let from = cb["from"]["id"].to_string();
        let data = cb["data"].as_str().unwrap_or("");
        let mut parts = data.splitn(3, ':');
        let (tag, id, answer) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let shown = if tag == "apr" && !id.is_empty() {
            engine.channel_callback(&ch.id, std::slice::from_ref(&from), id, answer == "y")
        } else {
            String::new()
        };
        let _ = t.call("answerCallbackQuery", json!({"callback_query_id": cb["id"], "text": shown})).await;
        return;
    }
    let Some(m) = u.get("message") else { return };
    let user = m["from"]["id"].to_string();
    let chat = m["chat"]["id"].to_string();
    // The user id and (for a group the person listed) the chat id are what the list is checked against.
    let senders = vec![user.clone(), chat.clone()];
    let text = m["text"].as_str().or_else(|| m["caption"].as_str()).unwrap_or("").to_string();
    let target = Target { conv: chat.clone(), reply: Value::Null };

    // The pairing code: `/start 123456`.
    if let Some(rest) = text.strip_prefix("/start") {
        let code = rest.trim();
        if !code.is_empty() && engine.channel_try_pair(&ch.id, &user, code) {
            let _ = tr.send(&target, "Fatto: ora sei nella lista e puoi scrivermi qui.", None).await;
            return;
        }
        if code.is_empty() && engine.channel_is_allowed(&ch.id, &senders) {
            let _ = tr.send(&target, "Ciao! Scrivimi pure: ti rispondo qui.", None).await;
            return;
        }
        // A wrong code, or a stranger: counted and ignored like any other message.
        engine.channel_note_ignored(&ch.id, &user);
        return;
    }

    // Attachments are downloaded only for someone on the list.
    let mut files = Vec::new();
    if engine.channel_is_allowed(&ch.id, &senders) {
        let mut wanted: Vec<(String, String, String, u64)> = Vec::new(); // (file_id, name, mime, size)
        if let Some(photo) = m["photo"].as_array().and_then(|p| p.last()) {
            wanted.push((
                photo["file_id"].as_str().unwrap_or("").into(),
                "foto.jpg".into(),
                "image/jpeg".into(),
                photo["file_size"].as_u64().unwrap_or(0),
            ));
        }
        if let Some(d) = m.get("document").filter(|d| d.is_object()) {
            wanted.push((
                d["file_id"].as_str().unwrap_or("").into(),
                d["file_name"].as_str().unwrap_or("documento").into(),
                d["mime_type"].as_str().unwrap_or("").into(),
                d["file_size"].as_u64().unwrap_or(0),
            ));
        }
        for (file_id, name, mime, size) in wanted.into_iter().filter(|w| !w.0.is_empty()) {
            if size > MAX_ATTACHMENT_BYTES as u64 {
                let _ = tr.send(&target, &format!("Non posso leggere «{name}»: è troppo grande."), None).await;
                continue;
            }
            match t.download(&file_id).await {
                Ok(bytes) => files.push(AttachmentUpload { name, mime, data: base64::engine::general_purpose::STANDARD.encode(bytes) }),
                Err(e) => {
                    let _ = tr.send(&target, &format!("Non riesco a scaricare «{name}»: {e}"), None).await;
                }
            }
        }
        // Voice notes, videos, stickers…: not supported, said in a few words.
        if files.is_empty()
            && text.trim().is_empty()
            && ["voice", "video", "audio", "sticker", "video_note", "animation"].iter().any(|k| m.get(*k).is_some())
        {
            let _ = tr.send(&target, "Per ora leggo solo testo, foto e documenti.", None).await;
            return;
        }
    }
    let msg = Inbound { conv: chat, senders, name: display_name(&m["from"]), text, files, reply: Value::Null };
    engine.channel_inbound(&ch.id, tr.clone(), msg).await;
}
