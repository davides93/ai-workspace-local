//! Core types shared by every layer of AI Workspace Local.
//!
//! * [`api`] — DTOs exposed over IPC/HTTP (mirrored in `apps/ui/src/api/types.ts`).
//! * [`events`] — AG-UI 1.0 run events (ADR 0004).
//! * [`schema`] — settings schemas used to generate configuration forms (ADR 0003).
//! * [`config`] — layered configuration: defaults → global → device → workspace.
//! * [`paths`] — data directories per platform.

pub mod api;
pub mod config;
pub mod error;
pub mod events;
pub mod ids;
pub mod paths;
pub mod schema;

pub use error::{Error, Result};

/// Crate version reported by `/api/system`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Current UTC time as RFC 3339 string (the wire format for timestamps).
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
