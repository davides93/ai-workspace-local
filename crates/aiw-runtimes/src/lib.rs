//! Inference layer (architecture §4): manifests, detection, adapters, sidecar supervisor.

pub mod adapters;
#[cfg(feature = "llama-embedded")]
pub mod embedded;
pub mod fallback;
pub mod install;
pub mod job;
pub mod manifest;
pub mod ollama;
pub mod openai;
#[cfg(not(target_os = "ios"))]
pub mod ortgenai;
pub mod registry;
pub mod scripted;
pub mod stt;
pub mod supervisor;
pub mod textproto;
pub mod trainer;
pub mod types;

pub use fallback::{FallbackBackend, FallbackResolver};
pub use manifest::RuntimeManifest;
pub use ollama::OllamaBackend;
pub use openai::OpenAiBackend;
pub use registry::{ModelResolver, RuntimeRegistry};
pub use scripted::ScriptedBackend;
pub use types::*;
