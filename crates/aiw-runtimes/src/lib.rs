//! Inference layer (architecture §4): manifests, detection, adapters, sidecar supervisor.

#[cfg(feature = "llama-embedded")]
pub mod embedded;
pub mod install;
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
pub mod types;

pub use manifest::RuntimeManifest;
pub use ollama::OllamaBackend;
pub use openai::OpenAiBackend;
pub use registry::{ModelResolver, RuntimeRegistry};
pub use scripted::ScriptedBackend;
pub use types::*;
