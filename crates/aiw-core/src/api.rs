//! Wire DTOs. Field names match `apps/ui/src/api/types.ts` exactly.

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
    /// Apple Silicon / mobile SoCs share memory between CPU and GPU.
    pub fn unified_memory(&self) -> bool {
        matches!(self.os.as_str(), "ios" | "android") || (self.os == "macos" && self.has_accelerator("metal"))
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
}

// ---------------------------------------------------------------- models

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub runtimes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadProgress {
    pub id: String,
    pub model_id: String,
    pub file: String,
    pub received: u64,
    pub total: u64,
    /// running | done | error | cancelled
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    /// user | assistant | tool | system
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WorkflowStep {
    Tool {
        id: String,
        tool: String,
        #[serde(default)]
        args: Map<String, Value>,
    },
    Agent {
        id: String,
        agent: String,
        prompt: String,
    },
    Llm {
        id: String,
        profile: String,
        prompt: String,
    },
    Approval {
        id: String,
        message: String,
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
            | WorkflowStep::Approval { id, .. }
            | WorkflowStep::Condition { id, .. } => id,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRun {
    pub id: String,
    pub workflow_id: String,
    pub status: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    pub steps: Vec<WorkflowStepRun>,
}

// ---------------------------------------------------------------- tools & MCP

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    /// builtin | mcp
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Map<String, Value>>,
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
