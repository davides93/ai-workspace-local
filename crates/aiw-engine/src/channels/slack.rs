//! Slack through Socket Mode: the app-level token (`xapp-…`) opens a WebSocket (`apps.connections.open`),
//! the events arrive there (`message.im` and `app_mention`) and every envelope is acknowledged at once;
//! answers go out with the bot token (`xoxb-…`) through `chat.postMessage`. Nothing has to be reachable
//! from the internet. When the connection drops it is opened again with a growing pause (the supervisor).
//!
//! The two tokens only go to the Slack API address (`https://slack.com/api`, or a fake server in the tests).

use super::{set_status, text, Engine, Inbound, Target, Transport};
use aiw_core::api::Channel;
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

pub(super) const DEFAULT_API: &str = "https://slack.com/api";
const LIMIT: usize = 3500;

pub(super) struct Slack {
    client: reqwest::Client,
    api: String,
    bot: String,
    app: String,
}

fn describe(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "Slack non risponde in tempo".into()
    } else if e.is_connect() {
        "non riesco a raggiungere Slack: controlla la connessione".into()
    } else {
        format!("errore di rete verso Slack: {e}")
    }
}

fn explain(code: &str) -> String {
    match code {
        "invalid_auth" | "not_authed" | "token_revoked" | "account_inactive" => {
            format!("Slack non accetta il token ({code}): ricontrollalo")
        }
        "missing_scope" => "al bot mancano dei permessi (scope): controlla la guida in docs/channels.md".to_string(),
        "channel_not_found" | "not_in_channel" => "il bot non può scrivere in quella conversazione".to_string(),
        "ratelimited" => "Slack chiede di rallentare".to_string(),
        other => format!("Slack risponde: {other}"),
    }
}

impl Slack {
    pub(super) fn new(engine: &Engine, ch: &Channel) -> Self {
        Self {
            client: reqwest::Client::builder().timeout(Duration::from_secs(20)).build().unwrap_or_default(),
            api: engine.channel_endpoints().slack_api.unwrap_or_else(|| DEFAULT_API.into()).trim_end_matches('/').to_string(),
            bot: ch.secrets.get("bot_token").cloned().unwrap_or_default(),
            app: ch.secrets.get("app_token").cloned().unwrap_or_default(),
        }
    }

    async fn call(&self, method: &str, token: &str, body: Value) -> Result<Value, String> {
        let resp = self
            .client
            .post(format!("{}/{method}", self.api))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| describe(e.without_url()))?;
        let v: Value = resp.json().await.map_err(|e| format!("risposta non valida da Slack ({})", e.without_url()))?;
        if v["ok"].as_bool() == Some(true) {
            Ok(v)
        } else {
            Err(explain(v["error"].as_str().unwrap_or("errore sconosciuto")))
        }
    }
}

#[async_trait]
impl Transport for Slack {
    async fn send(&self, to: &Target, text: &str, ask: Option<&str>) -> Result<(), String> {
        let safe = text::slack_escape(text);
        let mut body = json!({"channel": to.conv.split(':').next().unwrap_or(""), "text": safe, "mrkdwn": false, "unfurl_links": false});
        if let Some(thread) = to.reply["thread_ts"].as_str() {
            body["thread_ts"] = json!(thread);
        }
        if let Some(id) = ask {
            body["blocks"] = json!([
                {"type": "section", "text": {"type": "plain_text", "text": text, "emoji": false}},
                {"type": "actions", "elements": [
                    {"type": "button", "text": {"type": "plain_text", "text": "Sì"}, "style": "primary", "action_id": "apr_y", "value": id},
                    {"type": "button", "text": {"type": "plain_text", "text": "No"}, "style": "danger", "action_id": "apr_n", "value": id}
                ]}
            ]);
        }
        self.call("chat.postMessage", &self.bot, body).await.map(|_| ())
    }

    fn limit(&self) -> usize {
        LIMIT
    }
}

pub(super) async fn test(engine: &Engine, ch: &Channel) -> Result<String, String> {
    let s = Slack::new(engine, ch);
    let who = s.call("auth.test", &s.bot, json!({})).await?;
    // The app-level token is checked by asking for a connection address (nothing is connected).
    s.call("apps.connections.open", &s.app, json!({})).await.map_err(|e| format!("il token dell'app (xapp-…) non va: {e}"))?;
    Ok(format!(
        "Collegato a Slack come «{}» nell'area di lavoro «{}».",
        who["user"].as_str().unwrap_or("?"),
        who["team"].as_str().unwrap_or("?")
    ))
}

pub(super) async fn run(
    engine: &Engine,
    ch: &Channel,
    status: &Arc<Mutex<aiw_core::api::ChannelStatus>>,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let s = Slack::new(engine, ch);
    let me = s.call("auth.test", &s.bot, json!({})).await?;
    let bot_user = me["user_id"].as_str().unwrap_or("").to_string();
    let opened = s.call("apps.connections.open", &s.app, json!({})).await?;
    let url = opened["url"].as_str().ok_or("Slack non ha dato l'indirizzo della connessione")?.to_string();
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(20), tokio_tungstenite::connect_async(url))
        .await
        .map_err(|_| "Slack non risponde in tempo".to_string())?
        .map_err(|e| format!("connessione a Slack non riuscita: {e}"))?;
    let tr: Arc<dyn Transport> = Arc::new(Slack::new(engine, ch));
    let mut seen: VecDeque<String> = VecDeque::new();
    loop {
        let frame = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = ws.close(None).await;
                return Ok(());
            }
            f = ws.next() => f,
        };
        let msg = match frame {
            None => return Err("la connessione con Slack si è chiusa".into()),
            Some(Err(e)) => return Err(format!("la connessione con Slack è caduta: {e}")),
            Some(Ok(m)) => m,
        };
        let txt = match msg {
            Message::Text(t) => t.to_string(),
            Message::Ping(p) => {
                let _ = ws.send(Message::Pong(p)).await;
                continue;
            }
            Message::Close(_) => return Err("Slack ha chiuso la connessione".into()),
            _ => continue,
        };
        let Ok(env) = serde_json::from_str::<Value>(&txt) else { continue };
        // Every envelope is acknowledged first: Slack sends it again if not answered within three seconds.
        if let Some(id) = env["envelope_id"].as_str() {
            let _ = ws.send(Message::text(json!({"envelope_id": id}).to_string())).await;
        }
        match env["type"].as_str().unwrap_or("") {
            "hello" => set_status(status, "connected", format!("Collegato a Slack come «{}».", me["user"].as_str().unwrap_or("?"))),
            // Slack asks to reconnect (it does so now and then): not an error.
            "disconnect" => return Ok(()),
            "events_api" => handle_event(engine, ch, &tr, &env["payload"]["event"], &bot_user, &mut seen).await,
            "interactive" => handle_interactive(engine, ch, &env["payload"]),
            _ => {}
        }
    }
}

async fn handle_event(engine: &Engine, ch: &Channel, tr: &Arc<dyn Transport>, ev: &Value, bot_user: &str, seen: &mut VecDeque<String>) {
    let kind = ev["type"].as_str().unwrap_or("");
    let user = ev["user"].as_str().unwrap_or("");
    // The bot's own messages and the ones other bots or edits make are never answered.
    if user.is_empty() || user == bot_user || ev.get("bot_id").is_some() || ev.get("subtype").is_some() {
        return;
    }
    let is_dm = kind == "message" && ev["channel_type"].as_str() == Some("im");
    if !is_dm && kind != "app_mention" {
        return;
    }
    // The same message can arrive as `message` and as `app_mention`, or again after a retry.
    let key = format!("{}:{}", ev["channel"].as_str().unwrap_or(""), ev["ts"].as_str().unwrap_or(""));
    if seen.contains(&key) {
        return;
    }
    seen.push_back(key);
    if seen.len() > 200 {
        seen.pop_front();
    }
    let channel = ev["channel"].as_str().unwrap_or("");
    let mut body = ev["text"].as_str().unwrap_or("").to_string();
    if !bot_user.is_empty() {
        body = body.replace(&format!("<@{bot_user}>"), "").trim().to_string();
    }
    // In a channel the answer goes in the thread of the message.
    let (conv, reply) = if is_dm {
        (channel.to_string(), Value::Null)
    } else {
        let thread = ev["thread_ts"].as_str().or_else(|| ev["ts"].as_str()).unwrap_or("");
        (format!("{channel}:{thread}"), json!({"thread_ts": thread}))
    };
    let msg = Inbound { conv, senders: vec![user.to_string()], name: user.to_string(), text: body, files: vec![], reply };
    engine.channel_inbound(&ch.id, tr.clone(), msg).await;
}

fn handle_interactive(engine: &Engine, ch: &Channel, p: &Value) {
    if p["type"].as_str() != Some("block_actions") {
        return;
    }
    let user = p["user"]["id"].as_str().unwrap_or("").to_string();
    for a in p["actions"].as_array().cloned().unwrap_or_default() {
        let allow = match a["action_id"].as_str() {
            Some("apr_y") => true,
            Some("apr_n") => false,
            _ => continue,
        };
        if let Some(id) = a["value"].as_str() {
            engine.channel_callback(&ch.id, std::slice::from_ref(&user), id, allow);
        }
    }
}
