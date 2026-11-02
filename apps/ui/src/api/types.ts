// API contract shared by every transport (Tauri IPC, HTTP+SSE, mock).
// Mirrors the Rust types in crates/aiw-core/src/{api,events}.rs — keep them in sync.

export type Platform = "windows" | "macos" | "linux" | "android" | "ios" | "web";

export type AcceleratorKind =
  | "cpu" | "cuda" | "rocm" | "metal" | "vulkan" | "sycl" | "opencl"
  | "directml" | "npu" | "coreml" | "qnn" | "webgpu";

export interface Accelerator {
  kind: AcceleratorKind;
  name: string;
  vendor?: string;
  memory_mb?: number;
  driver?: string;
  details?: string;
}

export type DeviceTier = "xs" | "s" | "m" | "l" | "xl";

export interface DeviceProfile {
  id: string;
  name: string;
  os: Platform;
  os_version: string;
  arch: string;
  form_factor: "desktop" | "laptop" | "phone" | "tablet" | "server";
  cpu: { brand: string; cores: number; threads: number; features: string[] };
  memory_mb: number;
  available_memory_mb: number;
  accelerators: Accelerator[];
  tier: DeviceTier;
}

export interface SystemInfo {
  version: string;
  mode: "embedded" | "daemon" | "demo";
  data_dir: string;
  workspace_root: string;
  uptime_s: number;
  pairing_code?: string;
  device: DeviceProfile;
  peers: PeerDevice[];
}

export interface PeerDevice {
  id: string;
  name: string;
  os: Platform;
  address: string;
  status: "online" | "offline";
  shared_runtimes: string[];
}

// ---------- Settings schema (JSON Schema subset → generated forms) ----------
export interface FieldSchema {
  type: "string" | "integer" | "number" | "boolean" | "array";
  title?: string;
  description?: string;
  default?: unknown;
  enum?: (string | number)[];
  minimum?: number;
  maximum?: number;
  step?: number;
  items?: { type: "string" | "number" | "integer" };
  secret?: boolean;
  advanced?: boolean;
  placeholder?: string;
}

export interface SettingsSchema {
  type: "object";
  properties: Record<string, FieldSchema>;
  required?: string[];
}

// ---------- Runtimes ----------
export type RuntimeKind = "embedded" | "sidecar" | "system" | "remote";
export type RuntimeStatus =
  | "running" | "starting" | "available" | "installable" | "unsupported" | "stopped" | "error";

export interface RuntimeInfo {
  id: string;
  name: string;
  vendor?: string;
  kind: RuntimeKind;
  description: string;
  platforms: Platform[];
  accelerators: AcceleratorKind[];
  formats: string[];
  api: "openai" | "ollama" | "native";
  capabilities: string[];
  status: RuntimeStatus;
  status_detail?: string;
  endpoint?: string;
  loaded_models: string[];
  settings_schema: SettingsSchema;
  settings: Record<string, unknown>;
  install_hint?: string;
  homepage?: string;
}

// ---------- Models ----------
export interface LocalModel {
  id: string;
  name: string;
  path?: string;
  format: string;
  size_bytes: number;
  quant?: string;
  params_b?: number;
  context_length?: number;
  arch?: string;
  capabilities: string[];
  runtimes: string[];
  source?: string;
}

export type Fit = "great" | "ok" | "tight" | "no";

export interface CatalogModel {
  id: string;
  name: string;
  family: string;
  publisher: string;
  description: string;
  params_b: number;
  active_params_b?: number;
  format: string;
  quant: string;
  size_bytes: number;
  context_length: number;
  capabilities: string[];
  license: string;
  source: { kind: "hf"; repo: string; file: string } | { kind: "system"; runtime: string };
  min_memory_mb: number;
  tags: string[];
  fit: Fit;
  fit_detail: string;
  installed: boolean;
}

export interface DownloadProgress {
  id: string;
  model_id: string;
  file: string;
  received: number;
  total: number;
  status: "running" | "done" | "error" | "cancelled";
  error?: string;
}

// ---------- Profiles & agents ----------
export interface InferenceProfile {
  id: string;
  name: string;
  description?: string;
  runtime_id: string;
  model: string;
  params: { temperature?: number; top_p?: number; max_tokens?: number; context?: number };
}

export type PolicyMode = "plan" | "ask" | "auto-edit" | "auto" | "yolo";
export type SandboxMode = "read-only" | "workspace-write" | "off";

export interface AgentProfile {
  id: string;
  name: string;
  description: string;
  inference_profile: string;
  system_prompt: string;
  tools: string[];
  mode: PolicyMode;
  sandbox: SandboxMode;
  max_turns: number;
}

// ---------- Sessions, messages, runs ----------
export interface Session {
  id: string;
  title: string;
  agent_id: string;
  created_at: string;
  updated_at: string;
  last_status?: RunStatus;
  /** Present when the session works in its own git worktree (branch `aiw/<id>`). */
  worktree?: WorktreeInfo;
}

export interface WorktreeInfo {
  path: string;
  root: string;
  branch: string;
  base: string;
  repo: string;
}

export interface ChangedFile {
  status: string;
  path: string;
}

export interface WorktreeStatus extends WorktreeInfo {
  files: ChangedFile[];
  diff_stat: string;
  diff: string;
  truncated: boolean;
}

export type RunStatus = "running" | "waiting_approval" | "succeeded" | "failed" | "cancelled";

export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "tool" | "system";
  content: string;
  reasoning?: string;
  tool_calls?: ToolCall[];
  tool_call_id?: string;
  name?: string;
  created_at: string;
}

// AG-UI 1.0 event subset used by AIW (see docs/architecture/adr/0004-ag-ui-events.md)
export type AgEvent = { timestamp?: number } & (
  | { type: "RUN_STARTED"; threadId: string; runId: string }
  | { type: "RUN_FINISHED"; threadId: string; runId: string; result?: unknown }
  | { type: "RUN_ERROR"; message: string; code?: string }
  | { type: "STEP_STARTED"; stepName: string }
  | { type: "STEP_FINISHED"; stepName: string }
  | { type: "TEXT_MESSAGE_START"; messageId: string; role: "assistant" }
  | { type: "TEXT_MESSAGE_CONTENT"; messageId: string; delta: string }
  | { type: "TEXT_MESSAGE_END"; messageId: string }
  | { type: "REASONING_START"; messageId: string }
  | { type: "REASONING_MESSAGE_CONTENT"; messageId: string; delta: string }
  | { type: "REASONING_END"; messageId: string }
  | { type: "TOOL_CALL_START"; toolCallId: string; toolCallName: string; parentMessageId?: string }
  | { type: "TOOL_CALL_ARGS"; toolCallId: string; delta: string }
  | { type: "TOOL_CALL_END"; toolCallId: string }
  | { type: "TOOL_CALL_RESULT"; messageId: string; toolCallId: string; content: string }
  | { type: "STATE_SNAPSHOT"; snapshot: RunState }
  | { type: "SUBAGENT_STARTED"; subagentId: string; name: string }
  | { type: "SUBAGENT_FINISHED"; subagentId: string; result?: string }
  | { type: "CUSTOM"; name: string; value: unknown }
);

export interface RunState {
  todos?: { content: string; status: "pending" | "in_progress" | "completed" }[];
  usage?: { prompt_tokens: number; completion_tokens: number; tokens_per_s?: number; context_used?: number; context_max?: number };
  runtime?: string;
  model?: string;
}

export interface ApprovalRequest {
  id: string;
  run_id: string;
  tool: string;
  args: unknown;
  reason: string;
  risk: "low" | "medium" | "high";
}

// ---------- Workflows ----------
export type WorkflowTrigger =
  | { kind: "manual" }
  | { kind: "interval"; seconds: number }
  | { kind: "cron"; expr: string }
  | { kind: "file"; path: string }
  | { kind: "webhook"; secret?: string };

export type WorkflowStep =
  | { id: string; kind: "tool"; tool: string; args: Record<string, unknown> }
  | { id: string; kind: "agent"; agent: string; prompt: string }
  | { id: string; kind: "llm"; profile: string; prompt: string }
  | { id: string; kind: "approval"; message: string }
  | { id: string; kind: "condition"; if: string; then: string[]; else?: string[] };

export interface WorkflowDef {
  id: string;
  name: string;
  description?: string;
  enabled: boolean;
  trigger: WorkflowTrigger;
  inputs: Record<string, unknown>;
  steps: WorkflowStep[];
}

export interface WorkflowRun {
  id: string;
  workflow_id: string;
  status: RunStatus;
  started_at: string;
  finished_at?: string;
  steps: { id: string; status: "pending" | "running" | "done" | "failed" | "skipped" | "waiting"; output?: string; error?: string }[];
}

// ---------- Tools & MCP ----------
export type ToolRisk = "read" | "write" | "exec" | "network" | "computer";

export interface ToolInfo {
  name: string;
  description: string;
  source: "builtin" | "mcp";
  server?: string;
  risk: ToolRisk;
  enabled: boolean;
  available: boolean;
  unavailable_reason?: string;
  parameters: unknown;
}

export interface McpServer {
  id: string;
  name: string;
  transport: "stdio" | "http";
  command?: string;
  args?: string[];
  url?: string;
  enabled: boolean;
  status: "connected" | "disconnected" | "error";
  tools: string[];
  error?: string;
}

// ---------- Config & audit ----------
export interface ConfigLayer {
  name: "defaults" | "global" | "device" | "workspace";
  path?: string;
  values: Record<string, unknown>;
}

export interface ConfigView {
  layers: ConfigLayer[];
  effective: Record<string, unknown>;
  schema: SettingsSchema;
}

export interface AuditEntry {
  id: string;
  ts: string;
  run_id?: string;
  actor: "agent" | "user" | "workflow" | "system";
  action: string;
  tool?: string;
  decision?: "allow" | "ask" | "deny" | "approved" | "rejected";
  detail: string;
}
