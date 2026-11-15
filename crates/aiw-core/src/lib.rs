//! Core types shared by every layer of AI Workspace Local.
//!
//! * [`api`] — DTOs exposed over IPC/HTTP (mirrored in `apps/ui/src/shared/api/types.ts`).
//! * [`events`] — AG-UI 1.0 run events (ADR 0004).
//! * [`schema`] — settings schemas used to generate configuration forms (ADR 0003).
//! * [`config`] — layered configuration: defaults → global → device → workspace.
//! * [`paths`] — data directories per platform.
#![recursion_limit = "256"]

pub mod api;
pub mod config;
pub mod error;
pub mod events;
pub mod folders;
pub mod ids;
pub mod paths;
pub mod schema;
pub mod tlspin;

pub use error::{Error, Result};

/// Log directives that apply whatever `RUST_LOG` says (add them to the filter after reading it). The WebSocket library logs
/// the frames it sends at `trace`, and the frames to the controlled browser carry what the person types in the live view
/// (a password, a one-time code): they are never logged.
pub const QUIET_LOG_TARGETS: &[&str] = &["tungstenite=warn", "tokio_tungstenite=warn"];

/// Crate version reported by `/api/system`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Current UTC time as RFC 3339 string (the wire format for timestamps).
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
