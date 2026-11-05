//! AG-UI 1.0 events (subset) — ADR 0004. Serialized exactly as the AG-UI wire format:
//! `{"type":"TEXT_MESSAGE_CONTENT","messageId":"…","delta":"…"}`.

use crate::api::{ApprovalRequest, RunState};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
pub enum AgEvent {
    RunStarted {
        thread_id: String,
        run_id: String,
    },
    RunFinished {
        thread_id: String,
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
    },
    RunError {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    StepStarted {
        step_name: String,
    },
    StepFinished {
        step_name: String,
    },
    TextMessageStart {
        message_id: String,
        role: String,
    },
    TextMessageContent {
        message_id: String,
        delta: String,
    },
    TextMessageEnd {
        message_id: String,
    },
    ReasoningStart {
        message_id: String,
    },
    ReasoningMessageContent {
        message_id: String,
        delta: String,
    },
    ReasoningEnd {
        message_id: String,
    },
    ToolCallStart {
        tool_call_id: String,
        tool_call_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_message_id: Option<String>,
    },
    ToolCallArgs {
        tool_call_id: String,
        delta: String,
    },
    ToolCallEnd {
        tool_call_id: String,
    },
    ToolCallResult {
        message_id: String,
        tool_call_id: String,
        content: String,
    },
    StateSnapshot {
        snapshot: RunState,
    },
    SubagentStarted {
        subagent_id: String,
        name: String,
        /// Id of the run of the sub-agent: its events are at `/api/runs/{id}/events.json`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
    },
    SubagentFinished {
        subagent_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<String>,
    },
    Custom {
        name: String,
        value: Value,
    },
}

impl AgEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(self, AgEvent::RunFinished { .. } | AgEvent::RunError { .. })
    }
    pub fn approval_request(req: &ApprovalRequest) -> Self {
        AgEvent::Custom { name: "aiw.approval_request".into(), value: serde_json::to_value(req).unwrap_or(Value::Null) }
    }
    pub fn approval_resolved(id: &str, allowed: bool) -> Self {
        AgEvent::Custom { name: "aiw.approval_resolved".into(), value: serde_json::json!({ "id": id, "allowed": allowed }) }
    }
}

/// An event with its position in the run log and a millisecond timestamp.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeqEvent {
    pub seq: u64,
    pub event: TimedEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimedEvent {
    #[serde(flatten)]
    pub event: AgEvent,
    pub timestamp: i64,
}

impl TimedEvent {
    pub fn now(event: AgEvent) -> Self {
        Self { event, timestamp: chrono::Utc::now().timestamp_millis() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_in_ag_ui_wire_format() {
        let e = AgEvent::ToolCallStart { tool_call_id: "c1".into(), tool_call_name: "read_file".into(), parent_message_id: None };
        assert_eq!(serde_json::to_value(&e).unwrap(), json!({"type": "TOOL_CALL_START", "toolCallId": "c1", "toolCallName": "read_file"}));
        let t = TimedEvent { event: AgEvent::TextMessageContent { message_id: "m".into(), delta: "hi".into() }, timestamp: 5 };
        assert_eq!(
            serde_json::to_value(&t).unwrap(),
            json!({"type": "TEXT_MESSAGE_CONTENT", "messageId": "m", "delta": "hi", "timestamp": 5})
        );
    }

    #[test]
    fn parses_ag_ui_events() {
        let e: AgEvent = serde_json::from_value(json!({"type": "RUN_STARTED", "threadId": "t", "runId": "r"})).unwrap();
        assert_eq!(e, AgEvent::RunStarted { thread_id: "t".into(), run_id: "r".into() });
        assert!(!e.is_terminal());
    }
}
