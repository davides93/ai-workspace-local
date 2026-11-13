//! `channel_send`: an agent sends a message to one of the person's own addresses on a configured channel.
//!
//! The tool must be granted explicitly (a `*` in the agent's tool list does not include it), never
//! appears in a read-only run (proactive or from a channel), only writes to the people on the channel's
//! own list, and sends at most a few messages an hour.

use crate::{Engine, Inner};
use aiw_core::{Error, Result};
use aiw_tools::{Risk, Tool, ToolContext, ToolOutput};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

pub(super) fn register(engine: &Engine) {
    engine.0.tools.register(Arc::new(ChannelSend(Arc::downgrade(&engine.0))));
}

struct ChannelSend(Weak<Inner>);

#[async_trait::async_trait]
impl Tool for ChannelSend {
    fn name(&self) -> &str {
        "channel_send"
    }

    fn description(&self) -> &str {
        "Invia un messaggio di testo a te stesso su uno dei canali configurati (Telegram, Slack o email). Scrive solo a chi è nella lista del canale; non può contattare altre persone."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["channel", "text"],
            "properties": {
                "channel": {"type": "string", "description": "Nome o identificativo del canale"},
                "text": {"type": "string", "description": "Il messaggio, in testo semplice"},
                "to": {"type": "string", "description": "Destinatario, uno di quelli della lista del canale (predefinito: il primo)"}
            }
        })
    }

    fn risk(&self) -> Risk {
        Risk::Network
    }

    fn explicit_grant(&self) -> bool {
        true
    }

    /// The policy sees the channel, so a rule such as `channel_send(Telegram)` can allow or deny one channel.
    fn subject(&self, args: &Value) -> Option<String> {
        args.get("channel").and_then(|v| v.as_str()).map(String::from)
    }

    async fn call(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let engine = Engine(self.0.upgrade().ok_or_else(|| Error::Conflict("motore in arresto".into()))?);
        let channel = args.get("channel").and_then(|v| v.as_str()).ok_or_else(|| Error::Invalid("argomento `channel` mancante".into()))?;
        let text = args.get("text").and_then(|v| v.as_str()).ok_or_else(|| Error::Invalid("argomento `text` mancante".into()))?;
        let to = args.get("to").and_then(|v| v.as_str());
        Ok(ToolOutput::text(engine.channel_send_text(channel, to, text).await?))
    }
}
