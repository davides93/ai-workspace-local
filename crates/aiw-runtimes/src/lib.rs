//! Inference layer (architecture §4): manifests, detection, adapters, sidecar supervisor.

#[cfg(feature = "llama-embedded")]
pub mod embedded;
pub mod manifest;
pub mod openai;
pub mod registry;
pub mod scripted;
pub mod supervisor;
pub mod types;

pub use manifest::RuntimeManifest;
pub use openai::OpenAiBackend;
pub use registry::{ModelResolver, RuntimeRegistry};
pub use scripted::ScriptedBackend;
pub use types::*;
