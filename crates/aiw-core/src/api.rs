//! Wire DTOs. Field names match `apps/ui/src/shared/api/types.ts` exactly.

use crate::schema::SettingsSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ---------------------------------------------------------------- device

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Accelerator {
    /// cpu | cuda | rocm | metal | vulkan | sycl | opencl | directml | npu | coreml | qnn | webgpu
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuInfo {
    pub brand: String,
    pub cores: u32,
    pub threads: u32,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceProfile {
    pub id: String,
    pub name: String,
    /// windows | macos | linux | android | ios | web
    pub os: String,
    pub os_version: String,
    pub arch: String,
    /// desktop | laptop | phone | tablet | server
    pub form_factor: String,
    pub cpu: CpuInfo,
    pub memory_mb: u64,
    pub available_memory_mb: u64,
    pub accelerators: Vec<Accelerator>,
    /// xs | s | m | l | xl
    pub tier: String,
}

impl DeviceProfile {
    pub fn has_accelerator(&self, kind: &str) -> bool {
        self.accelerators.iter().any(|a| a.kind == kind)
    }
    /// Largest dedicated accelerator memory (VRAM), if any.
    pub fn vram_mb(&self) -> u64 {
        self.accelerators.iter().filter(|a| a.kind != "cpu").filter_map(|a| a.memory_mb).max().unwrap_or(0)
    }
    /// Apple Silicon / mobile SoCs share memory between CPU and GPU. An Intel Mac does not: it has
    /// a Metal GPU too, but with its own (or carved-out) memory, no Neural Engine and no MLX.
    pub fn unified_memory(&self) -> bool {
        matches!(self.os.as_str(), "ios" | "android") || (self.os == "macos" && self.arch == "aarch64" && self.has_accelerator("metal"))
    }
}

/// Voice mode: which speech-to-text backend would be used, and the voice settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VoiceStatus {
    pub available: bool,
    /// Runtime that transcribes (`whisper-server`, `localai`, `aiw-peer`…).
    pub runtime: Option<String>,
    /// Model it uses, when the runtime needs one.
    pub model: Option<String>,
    /// Human description, e.g. "whisper.cpp server · ggml-small-q8_0".
    pub backend: Option<String>,
    /// Why voice is unavailable, or what to do to enable it.
    pub detail: Option<String>,
    /// ISO 639-1 code or `auto`.
    pub language: String,
    /// Preferred local system voice for reading replies aloud (empty = first one for the language).
    pub tts_voice: String,
    /// Installed speech-to-text models.
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
    pub backend: String,
    /// Length of the audio, when known (WAV).
    pub duration_ms: Option<u64>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerDevice {
    pub id: String,
    pub name: String,
    pub os: String,
    pub address: String,
    pub status: String,
    pub shared_runtimes: Vec<String>,
}

/// An agent that runs elsewhere and speaks A2A (another device, an orchestrator, a cloud agent). Teams and
/// agents delegate to it as `remote:<id>`. The API shows [`SECRET_MASK`] in place of the token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteAgent {
    pub id: String,
    pub name: String,
    /// A2A base URL (`http://host:7777`) or the URL of its Agent Card.
    pub url: String,
    #[serde(default)]
    pub token: String,
    /// The skill (agent) to ask for: sent as `metadata.agent`. Empty: the remote default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(default)]
    pub description: String,
    /// True for a paired device, which is offered automatically and is managed from the devices list.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub version: String,
    /// embedded | daemon | demo
    pub mode: String,
    pub data_dir: String,
    pub workspace_root: String,
    pub uptime_s: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing_code: Option<String>,
    pub device: DeviceProfile,
    pub peers: Vec<PeerDevice>,
}

// ---------------------------------------------------------------- runtimes

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// embedded | sidecar | system | remote
    pub kind: String,
    pub description: String,
    pub platforms: Vec<String>,
    pub accelerators: Vec<String>,
    pub formats: Vec<String>,
    /// openai | ollama | native
    pub api: String,
    pub capabilities: Vec<String>,
    /// running | starting | available | installable | unsupported | stopped | error
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub loaded_models: Vec<String>,
    pub settings_schema: SettingsSchema,
    pub settings: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_hint: Option<String>,
    /// AIW can install this runtime by itself on this OS (a pip or Homebrew recipe exists).
    #[serde(default)]
    pub install_auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
}

// ---------------------------------------------------------------- models

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub format: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_b: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    pub capabilities: Vec<String>,
    /// Ids of `runtime_compat`, available runtimes first.
    pub runtimes: Vec<String>,
    /// Exact provenance (`provider:model@revision`, `ollama:qwen3:4b`, …) when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// aiw | ollama | lmstudio | hf-cache | manual
    #[serde(default)]
    pub origin: String,
    /// Can be deleted from `models.dir` through the API (never true for other apps' stores).
    #[serde(default)]
    pub removable: bool,
    #[serde(default)]
    pub runtime_compat: Vec<RuntimeCompat>,
    /// great | ok | tight | no — computed for the current device
    #[serde(default)]
    pub fit: String,
    #[serde(default)]
    pub fit_detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ModelSource {
    Hf { repo: String, file: String },
    System { runtime: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub family: String,
    pub publisher: String,
    pub description: String,
    pub params_b: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_params_b: Option<f64>,
    pub format: String,
    pub quant: String,
    pub size_bytes: u64,
    pub context_length: u64,
    pub capabilities: Vec<String>,
    pub license: String,
    pub source: ModelSource,
    pub min_memory_mb: u64,
    pub tags: Vec<String>,
    /// great | ok | tight | no — computed for the current device
    #[serde(default)]
    pub fit: String,
    #[serde(default)]
    pub fit_detail: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub runtime_compat: Vec<RuntimeCompat>,
    /// False for system models (Apple FM, Gemini Nano, Phi Silica): nothing to download.
    #[serde(default)]
    pub downloadable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DownloadProgress {
    pub id: String,
    pub model_id: String,
    /// Current file.
    pub file: String,
    /// Aggregated over every file of the job.
    pub received: u64,
    pub total: u64,
    /// queued | running | verifying | done | error | cancelled
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model_ref: String,
    #[serde(default)]
    pub variant: String,
    #[serde(default)]
    pub files_total: u32,
    #[serde(default)]
    pub files_done: u32,
    #[serde(default)]
    pub speed_bps: u64,
    /// Destination folder (absolute).
    #[serde(default)]
    pub dest: String,
}

// ---------------------------------------------------------------- model hub (docs/architecture/model-hub.md §3)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelProviderInfo {
    /// catalog | hf | modelscope | ollama | url
    pub id: String,
    pub name: String,
    pub description: String,
    pub homepage: String,
    /// search | files | auth | multi_file
    pub capabilities: Vec<String>,
    /// Formats the provider can supply: gguf, mlx, onnx, safetensors, litertlm, …
    pub formats: Vec<String>,
    /// none | optional | required
    pub auth: String,
    pub auth_configured: bool,
    /// Effective base URL ("" when not applicable).
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteModel {
    pub provider: String,
    /// Reference inside the provider: `Qwen/Qwen3-4B-GGUF`, `qwen3:4b`, `https://…`.
    pub id: String,
    pub name: String,
    pub publisher: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// chat | tools | vision | embeddings | reasoning (deduced from tags and pipeline)
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub likes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default)]
    pub gated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_b: Option<f64>,
    /// Web page of the model ("" when there is none).
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SearchPage {
    pub items: Vec<RemoteModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteFile {
    /// Path inside the repository.
    pub path: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// weights | mmproj | config | tokenizer | other
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelVariant {
    /// Stable key inside the model: `Q4_K_M`, `repo`, `cpu-int4`, `latest`.
    pub id: String,
    pub label: String,
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quant: Option<String>,
    /// Every shard plus the mmproj, or every file of the repository / sub-folder.
    pub files: Vec<RemoteFile>,
    pub size_bytes: u64,
    /// great | ok | tight | no
    #[serde(default)]
    pub fit: String,
    #[serde(default)]
    pub fit_detail: String,
    /// At most one variant per model.
    #[serde(default)]
    pub recommended: bool,
    #[serde(default)]
    pub runtimes: Vec<RuntimeCompat>,
    #[serde(default)]
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteModelDetail {
    pub model: RemoteModel,
    /// Commit sha, or `main` / `master` / tag.
    pub revision: String,
    pub variants: Vec<ModelVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeCompat {
    pub id: String,
    pub name: String,
    /// Installed or registered on this device.
    pub available: bool,
    /// Why it is not available ("solo macOS", "da installare", …) — empty when available.
    #[serde(default)]
    pub reason: String,
}

/// Body of `POST /api/models/pull`: untagged, the first shape that matches wins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PullRequest {
    Hub {
        provider: String,
        model: String,
        /// Empty or absent: the recommended variant (or the only one).
        #[serde(default)]
        variant: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
    },
    Catalog {
        id: String,
    },
    /// v0.3 compatibility: one file of a Hugging Face repository.
    HfFile {
        repo: String,
        file: String,
    },
    Url {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
    },
}

// ---------------------------------------------------------------- profiles & agents

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct InferenceParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InferenceProfile {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub runtime_id: String,
    pub model: String,
    #[serde(default)]
    pub params: InferenceParams,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub inference_profile: String,
    #[serde(default)]
    pub system_prompt: String,
    /// Tool names; `"*"` means every enabled tool.
    pub tools: Vec<String>,
    /// plan | ask | auto-edit | auto | yolo
    pub mode: String,
    /// read-only | workspace-write | off
    pub sandbox: String,
    pub max_turns: u32,
    /// JSON Schema (a subset, see `aiw_agent::schema`) of the final answer: when set, the answer
    /// must be JSON that conforms to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    /// Limits of one run of this agent, sub-agents included; a missing value falls back to the
    /// `agents.budget.*` settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetLimits>,
}

/// Run budget: every limit is optional, and a set one is at least 1.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BudgetLimits {
    /// Prompt + completion tokens summed over every model call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Model calls, summed over the run and its sub-agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_seconds: Option<u64>,
}

/// A team of agents used in chat like a single agent (session agent id `team:<id>`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// supervisor | parallel | pipeline
    pub strategy: String,
    /// Agent id that orchestrates the members; required for the `supervisor` strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervisor: Option<String>,
    pub members: Vec<TeamMember>,
    /// Agent that merges the member results for the `parallel` strategy (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetLimits>,
    /// Schema of the final answer, same subset as [`AgentProfile::output_schema`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamMember {
    /// A local agent id, or `remote:<id>` for a remote agent.
    pub agent: String,
    /// What this member is for; shown to the supervisor as the sub-agent description.
    #[serde(default)]
    pub role: String,
}

// ---------------------------------------------------------------- sessions & messages

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub agent_id: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    /// Set when the session works in its own git worktree instead of the shared workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeInfo>,
}

/// A session isolated in a git worktree on branch `aiw/<id>`: parallel tasks never touch the
/// user's checkout until the changes are reviewed and applied.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorktreeInfo {
    /// Worktree checkout.
    pub path: String,
    /// Directory the session works in (the workspace's sub-folder inside the worktree).
    pub root: String,
    pub branch: String,
    /// Commit the session diff is measured against; it moves forward after each apply.
    pub base: String,
    /// Top level of the user's checkout.
    pub repo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChangedFile {
    /// git status letter: A, M, D, R…
    pub status: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorktreeStatus {
    #[serde(flatten)]
    pub info: WorktreeInfo,
    pub files: Vec<ChangedFile>,
    pub diff_stat: String,
    pub diff: String,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON-encoded arguments, as produced by the model.
    pub arguments: String,
}

/// `name` of the stored `system` message that records how a run ended in error (`content` is the
/// message the UI showed as RUN_ERROR). It is part of the chat history, never of the model's
/// context, and clients that do not know it ignore a `system` message.
pub const RUN_ERROR_MESSAGE: &str = "aiw.run_error";
/// `name` of a user message that a `UserPromptSubmit` hook blocked: kept for the chat history,
/// never sent to the model.
pub const BLOCKED_PROMPT_MESSAGE: &str = "aiw.blocked_prompt";
/// `name` of the messages written by a handoff to another agent: the user message that opens the
/// next agent's run (`[Passaggio da …] summary`, sent to the model) and the `system` notes about a
/// handoff that was not carried out (history only).
pub const HANDOFF_MESSAGE: &str = "aiw.handoff";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    /// user | assistant | tool | system. A `system` message named [`RUN_ERROR_MESSAGE`] is the
    /// error that ended a run.
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub created_at: String,
}

/// Longest note accepted with a rating.
pub const MAX_FEEDBACK_NOTE_CHARS: usize = 2000;

/// The user's rating of an assistant message (one per message).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Feedback {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    /// The run that produced the message, when it can be told.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// 1 (thumbs up) or -1 (thumbs down).
    pub rating: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// When the rating was last set.
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Todo {
    pub content: String,
    /// pending | in_progress | completed
    pub status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_per_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_used: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_max: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todos: Option<Vec<Todo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// State shared by the agents of a team run (the `team_state` tool): a JSON object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalRequest {
    pub id: String,
    pub run_id: String,
    pub tool: String,
    pub args: Value,
    pub reason: String,
    /// low | medium | high
    pub risk: String,
}

// ---------------------------------------------------------------- workflows

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WorkflowTrigger {
    Manual,
    Interval {
        seconds: u64,
    },
    Cron {
        expr: String,
    },
    File {
        path: String,
    },
    /// `POST /hooks/{id}` with the secret (header `X-AIW-Secret` or `?secret=`) or a GitHub-style
    /// `X-Hub-Signature-256` HMAC of the body. The JSON body becomes the workflow inputs.
    Webhook {
        #[serde(default)]
        secret: String,
    },
}

/// The answer an `approval` step takes when nobody answers in time.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDefault {
    #[default]
    Deny,
    Allow,
}

impl ApprovalDefault {
    fn is_deny(&self) -> bool {
        *self == ApprovalDefault::Deny
    }
}

/// How an approval request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalOutcome {
    Allowed,
    Denied,
    /// Nobody answered within the time limit.
    TimedOut,
}

/// What a workflow does when a step fails (after its retries).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OnError {
    /// The run fails (default).
    #[default]
    Fail,
    /// The step stays `failed` but the run goes on; `{{steps.<id>.error}}` holds the error.
    Continue,
}

impl OnError {
    fn is_fail(&self) -> bool {
        *self == OnError::Fail
    }
}

pub const MAX_STEP_RETRY: u32 = 5;
pub const MAX_RETRY_DELAY_S: u64 = 300;
pub const MAX_STEP_TIMEOUT_S: u64 = 86_400;
pub const DEFAULT_RETRY_DELAY_S: u64 = 2;

/// How many sub-workflows can be nested (a top-level run is depth 0).
pub const MAX_SUBWORKFLOW_DEPTH: u32 = 3;
pub const MAX_FOREACH_PARALLEL: u32 = 8;
pub const MAX_FOREACH_ITEMS: usize = 200;

fn one() -> u32 {
    1
}
fn is_one(n: &u32) -> bool {
    *n == 1
}
fn default_retry_delay() -> u64 {
    DEFAULT_RETRY_DELAY_S
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}
fn is_default_delay(n: &u64) -> bool {
    *n == DEFAULT_RETRY_DELAY_S
}

/// Options shared by the executable steps (`tool`, `agent`, `llm`, ...). Every field is optional
/// and the defaults reproduce the behaviour of a step without options; defaults are not serialized.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepOptions {
    /// Extra attempts after the first failure (0..=5).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retry: u32,
    /// Seconds to wait between attempts (0..=300).
    #[serde(default = "default_retry_delay", skip_serializing_if = "is_default_delay")]
    pub retry_delay_s: u64,
    /// Time limit of each attempt in seconds (1..=86400).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_s: Option<u64>,
    #[serde(default, skip_serializing_if = "OnError::is_fail")]
    pub on_error: OnError,
}

impl Default for StepOptions {
    fn default() -> Self {
        Self { retry: 0, retry_delay_s: DEFAULT_RETRY_DELAY_S, timeout_s: None, on_error: OnError::Fail }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WorkflowStep {
    Tool {
        id: String,
        tool: String,
        #[serde(default)]
        args: Map<String, Value>,
        #[serde(default, flatten)]
        opts: StepOptions,
    },
    Agent {
        id: String,
        agent: String,
        prompt: String,
        #[serde(default, flatten)]
        opts: StepOptions,
    },
    Llm {
        id: String,
        profile: String,
        prompt: String,
        #[serde(default, flatten)]
        opts: StepOptions,
    },
    /// Runs `do` once for every entry of `items` (a JSON array, or one entry per non-empty line),
    /// `parallel` at a time. `{{item}}`, `{{item.field}}` and `{{index}}` are available in `do`.
    /// The output is the JSON array of the outputs, in the order of the entries.
    Foreach {
        id: String,
        items: String,
        /// A single inline `tool`, `agent` or `llm` step.
        #[serde(rename = "do")]
        body: Box<WorkflowStep>,
        /// How many entries run at the same time (1..=8).
        #[serde(default = "one", skip_serializing_if = "is_one")]
        parallel: u32,
        #[serde(default, flatten)]
        opts: StepOptions,
    },
    /// Runs inline `tool`, `agent` and `llm` steps all at once. The output is the JSON object
    /// `{inner_id: output}`; each inner output is also `{{steps.<inner_id>.output}}`.
    Parallel {
        id: String,
        steps: Vec<WorkflowStep>,
        #[serde(default, flatten)]
        opts: StepOptions,
    },
    /// Starts another workflow and waits for it. The output is the output of the last step of the
    /// child that finished (`done`); a failed or cancelled child is an error of this step. The child
    /// can be a disabled workflow (enabling only concerns the triggers) and runs at most 3 levels deep.
    Workflow {
        id: String,
        /// Id of the workflow to run.
        workflow: String,
        /// Inputs of the child; the values can use placeholders.
        #[serde(default)]
        inputs: Map<String, Value>,
        #[serde(default, flatten)]
        opts: StepOptions,
    },
    Approval {
        id: String,
        message: String,
        /// Seconds to wait for the answer (1..=86400); without it the step waits for ever.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_s: Option<u64>,
        /// What counts as the answer when the time is up.
        #[serde(default, rename = "default", skip_serializing_if = "ApprovalDefault::is_deny")]
        on_timeout: ApprovalDefault,
    },
    Condition {
        id: String,
        #[serde(rename = "if")]
        condition: String,
        then: Vec<String>,
        #[serde(default, rename = "else", skip_serializing_if = "Option::is_none")]
        otherwise: Option<Vec<String>>,
    },
}

impl WorkflowStep {
    pub fn id(&self) -> &str {
        match self {
            WorkflowStep::Tool { id, .. }
            | WorkflowStep::Agent { id, .. }
            | WorkflowStep::Llm { id, .. }
            | WorkflowStep::Foreach { id, .. }
            | WorkflowStep::Parallel { id, .. }
            | WorkflowStep::Workflow { id, .. }
            | WorkflowStep::Approval { id, .. }
            | WorkflowStep::Condition { id, .. } => id,
        }
    }

    /// The inline steps of a `foreach` (its `do`) or `parallel`.
    pub fn inner(&self) -> Vec<&WorkflowStep> {
        match self {
            WorkflowStep::Foreach { body, .. } => vec![body.as_ref()],
            WorkflowStep::Parallel { steps, .. } => steps.iter().collect(),
            _ => vec![],
        }
    }

    /// The step and its inline steps.
    pub fn with_inner(&self) -> Vec<&WorkflowStep> {
        std::iter::once(self).chain(self.inner()).collect()
    }

    /// The retry/timeout/error options of the steps that have them.
    pub fn opts(&self) -> Option<&StepOptions> {
        match self {
            WorkflowStep::Tool { opts, .. }
            | WorkflowStep::Agent { opts, .. }
            | WorkflowStep::Llm { opts, .. }
            | WorkflowStep::Foreach { opts, .. }
            | WorkflowStep::Parallel { opts, .. }
            | WorkflowStep::Workflow { opts, .. } => Some(opts),
            WorkflowStep::Approval { .. } | WorkflowStep::Condition { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDef {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    pub trigger: WorkflowTrigger,
    #[serde(default)]
    pub inputs: Map<String, Value>,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowStepRun {
    pub id: String,
    /// pending | running | done | failed | skipped | waiting
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Attempts made so far, only for steps with `retry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRun {
    pub id: String,
    pub workflow_id: String,
    pub status: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// The run of the workflow that started this one as a sub-workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    pub steps: Vec<WorkflowStepRun>,
}

// ---------------------------------------------------------------- checkpoints

/// What a rewind of a run would do: the originals saved before the run overwrote files, and the
/// files the run created. Paths are relative to `root` (the workspace of the run).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckpointInfo {
    pub run_id: String,
    /// Workspace root of the run; absent for a checkpoint made before rewind existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Files that would be put back to their original content.
    pub restore: Vec<String>,
    /// Files the run created, that would be deleted.
    pub delete: Vec<String>,
    /// Files outside the workspace (or of a checkpoint without a root): listed, never touched.
    pub external: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RewindResult {
    pub run_id: String,
    pub restored: Vec<String>,
    pub deleted: Vec<String>,
    pub external: Vec<String>,
    /// Files that could not be restored or deleted, with the reason.
    pub failed: Vec<String>,
}

// ---------------------------------------------------------------- tools & MCP

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    /// builtin | mcp | internal. `internal` tools (todo, agent, skill, render_ui, code) are
    /// implemented by the agent loop itself: an agent profile can list them, but they have no
    /// on/off switch (`PUT /api/tools/{name}` answers 400) and the MCP server does not expose them.
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// read | write | exec | network | computer
    pub risk: String,
    pub enabled: bool,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    pub parameters: Value,
}

/// What the API shows instead of a secret (runtime settings, MCP headers and environment). A
/// client that sends it back means "keep the stored value".
pub const SECRET_MASK: &str = "••••••";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpServerConfig {
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// stdio | http
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Environment of the `stdio` process. Reads show [`SECRET_MASK`] for the values that look like secrets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Map<String, Value>>,
    /// Extra HTTP headers for the `http` transport (e.g. `Authorization` for a paired device).
    /// Reads show [`SECRET_MASK`] for the values that look like secrets (see the engine).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Map<String, Value>>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    #[serde(flatten)]
    pub config: McpServerConfig,
    /// connected | disconnected | error
    pub status: String,
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------- config & audit

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigLayerView {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub values: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigView {
    pub layers: Vec<ConfigLayerView>,
    pub effective: Map<String, Value>,
    pub schema: SettingsSchema,
    /// Keys of the workspace layer (repository content) that are ignored because they would
    /// loosen security or set machine-level options.
    #[serde(default)]
    pub ignored_workspace_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditEntry {
    pub id: String,
    pub ts: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// agent | user | workflow | system
    pub actor: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// allow | ask | deny | approved | rejected
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    pub detail: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pull_request_takes_the_first_shape_that_matches() {
        let p = |v: Value| serde_json::from_value::<PullRequest>(v);
        assert_eq!(
            p(json!({"provider": "hf", "model": "Qwen/Qwen3-4B-GGUF", "variant": "Q4_K_M", "revision": "abc"})).unwrap(),
            PullRequest::Hub {
                provider: "hf".into(),
                model: "Qwen/Qwen3-4B-GGUF".into(),
                variant: "Q4_K_M".into(),
                revision: Some("abc".into())
            }
        );
        // No variant: the recommended one.
        assert_eq!(
            p(json!({"provider": "ollama", "model": "qwen3"})).unwrap(),
            PullRequest::Hub { provider: "ollama".into(), model: "qwen3".into(), variant: String::new(), revision: None }
        );
        assert_eq!(p(json!({"id": "qwen3-4b"})).unwrap(), PullRequest::Catalog { id: "qwen3-4b".into() });
        assert_eq!(p(json!({"repo": "a/b", "file": "c.gguf"})).unwrap(), PullRequest::HfFile { repo: "a/b".into(), file: "c.gguf".into() });
        assert_eq!(
            p(json!({"url": "https://x/y.gguf", "sha256": "ab"})).unwrap(),
            PullRequest::Url { url: "https://x/y.gguf".into(), file_name: None, sha256: Some("ab".into()) }
        );
        // Order: Hub wins over Catalog when both would match.
        assert!(matches!(p(json!({"provider": "hf", "model": "a/b", "variant": "Q4", "id": "x"})).unwrap(), PullRequest::Hub { .. }));
        assert!(p(json!({"nonsense": 1})).is_err());
        assert!(p(json!({"provider": "hf"})).is_err());
    }

    #[test]
    fn model_types_stay_compatible_with_v03_payloads() {
        // Payloads written before the hub (no new fields) still parse, with defaults.
        let m: LocalModel = serde_json::from_value(
            json!({"id": "a", "name": "A", "format": "gguf", "size_bytes": 1, "capabilities": ["chat"], "runtimes": ["llama-server"]}),
        )
        .unwrap();
        assert_eq!((m.origin.as_str(), m.removable, m.runtime_compat.len(), m.fit.as_str()), ("", false, 0, ""));
        let d: DownloadProgress =
            serde_json::from_value(json!({"id": "d", "model_id": "m", "file": "f", "received": 1, "total": 2, "status": "running"}))
                .unwrap();
        assert_eq!((d.files_total, d.files_done, d.speed_bps, d.provider.as_str()), (0, 0, 0, ""));
        let c = serde_json::to_value(RuntimeCompat { id: "x".into(), name: "X".into(), available: false, reason: "da installare".into() })
            .unwrap();
        assert_eq!(c, json!({"id": "x", "name": "X", "available": false, "reason": "da installare"}));
        // Optional fields are omitted, not null, on the wire.
        let v = serde_json::to_value(SearchPage { items: vec![], next_cursor: None }).unwrap();
        assert_eq!(v, json!({"items": []}));
        let r = serde_json::to_value(DownloadProgress { status: "queued".into(), ..Default::default() }).unwrap();
        assert!(r.get("error").is_none());
        for k in ["provider", "model_ref", "variant", "files_total", "files_done", "speed_bps", "dest"] {
            assert!(r.get(k).is_some(), "{k}");
        }
    }

    #[test]
    fn workflow_steps_roundtrip_with_ui_shape() {
        let v = json!({
            "id": "wf", "name": "Test", "enabled": true,
            "trigger": {"kind": "cron", "expr": "0 8 * * 1-5"},
            "inputs": {"folder": "~/Downloads"},
            "steps": [
                {"id": "scan", "kind": "tool", "tool": "list_dir", "args": {"path": "{{inputs.folder}}"}},
                {"id": "ok", "kind": "approval", "message": "Procedo?"},
                {"id": "c", "kind": "condition", "if": "x contains y", "then": ["scan"]}
            ]
        });
        let wf: WorkflowDef = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(wf.steps.len(), 3);
        assert_eq!(wf.steps[2].id(), "c");
        let back = serde_json::to_value(&wf).unwrap();
        assert_eq!(back["steps"][2]["if"], "x contains y");
        assert_eq!(back["trigger"]["kind"], "cron");
    }

    #[test]
    fn mcp_server_info_is_flat() {
        let info = McpServerInfo {
            config: McpServerConfig {
                id: "m".into(),
                name: "fs".into(),
                transport: "stdio".into(),
                command: Some("npx".into()),
                args: None,
                url: None,
                env: None,
                headers: None,
                enabled: true,
            },
            status: "connected".into(),
            tools: vec!["read".into()],
            error: None,
        };
        let v = serde_json::to_value(info).unwrap();
        assert_eq!(v["name"], "fs");
        assert_eq!(v["status"], "connected");
    }
}
