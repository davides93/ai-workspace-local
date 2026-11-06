// API contract shared by every transport (Tauri IPC, HTTP+SSE, mock).
// Mirrors the Rust types in crates/aiw-core/src/{api,events}.rs and the model hub contract in
// docs/architecture/model-hub.md — keep them in sync.

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

export interface DiscoveredDevice {
  id: string;
  name: string;
  os: string;
  tier: string;
  version: string;
  address: string;
  last_seen: string;
}

export interface MeshState {
  /** mDNS discovery running (the LAN server is on). */
  discovery: boolean;
  /** This device's address on the LAN, when reachable. */
  address: string | null;
  discovered: DiscoveredDevice[];
  peers: PeerDevice[];
}

/** Voice mode: which speech-to-text backend would be used, and the voice settings. */
export interface VoiceStatus {
  available: boolean;
  runtime: string | null;
  model: string | null;
  /** e.g. "whisper.cpp server · ggml-small-q8_0" */
  backend: string | null;
  /** Why voice is unavailable, or what to do to enable it. */
  detail: string | null;
  /** ISO 639-1 code or "auto". */
  language: string;
  /** Preferred local system voice for replies ("" = first one for the language). */
  tts_voice: string;
  models: string[];
}

export interface Transcript {
  text: string;
  language: string | null;
  backend: string;
  duration_ms: number | null;
  elapsed_ms: number;
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
  /** `none`: the runtime has no HTTP API of its own (embedded or bridged in-process). */
  api: "openai" | "ollama" | "native" | "none";
  capabilities: string[];
  status: RuntimeStatus;
  status_detail?: string;
  endpoint?: string;
  loaded_models: string[];
  settings_schema: SettingsSchema;
  settings: Record<string, unknown>;
  install_hint?: string;
  /** AIW can install this runtime by itself on this device (pip into its own environment, Homebrew). */
  install_auto?: boolean;
  homepage?: string;
}

/** What installing a runtime takes here (GET /api/runtimes/:id/install-plan). */
export interface InstallPlan {
  runtime_id: string;
  runtime_name: string;
  /** A recipe exists for this OS; otherwise only `hint` (manual instructions) applies. */
  automatic: boolean;
  steps: { title: string; command: string }[];
  prerequisites: { name: string; ok: boolean; hint?: string }[];
  hint?: string;
  homepage?: string | null;
  note: string;
}

/** An installation in progress or finished (POST/GET /api/runtimes/:id/install). */
export interface InstallJob {
  runtime_id: string;
  state: "running" | "done" | "failed";
  /** 1-based step that is running (or ran last). */
  step: number;
  steps: number;
  log: string[];
  error?: string;
}

// ---------- Updates ----------
export type UpdateChannel = "alpha" | "beta" | "rc" | "stable";
export type UpdateState = "idle" | "checking" | "downloading" | "ready" | "installing" | "error";

export interface ReleaseInfo {
  tag: string;
  version: string;
  channel: UpdateChannel;
  /** Release notes (Markdown) as published on GitHub. */
  notes: string;
  published_at?: string | null;
  page: string;
  /** Installer for this OS; missing when the release has none for it. */
  asset?: { name: string; size: number; url: string } | null;
}

/** GET /api/update — current version, channel in use and what the last check found. */
export interface UpdateStatus {
  current: string;
  /** The channel in use (what `auto` resolved to). */
  channel: UpdateChannel;
  /** The setting as saved: auto | stable | beta | alpha. */
  configured: string;
  state: UpdateState;
  available?: ReleaseInfo | null;
  checked_at?: string | null;
  received: number;
  total: number;
  /** Verified installer on disk. */
  file?: string | null;
  /** What to do next after the installer was started. */
  message?: string | null;
  error?: string | null;
}

// ---------- Models ----------
export type Fit = "great" | "ok" | "tight" | "no";

/** Can this runtime run a model/variant on the current device? (`reason` explains a "no"). */
export interface RuntimeCompat {
  id: string;
  name: string;
  available: boolean;
  reason: string;
}

export type ModelOrigin = "aiw" | "ollama" | "lmstudio" | "hf-cache" | "manual";

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
  /** Ids of `runtime_compat`, available runtimes first. */
  runtimes: string[];
  source?: string;
  /** Empty (`""`, older stores and hand-copied files) is read as `manual`. */
  origin: ModelOrigin | "";
  /** Only models inside `models.dir` can be deleted from the UI. */
  removable: boolean;
  runtime_compat: RuntimeCompat[];
  fit: Fit | "";
  fit_detail: string;
}

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
  runtime_compat: RuntimeCompat[];
  /** False for system models (nothing to download). */
  downloadable: boolean;
}

export type DownloadStatus = "queued" | "running" | "verifying" | "done" | "error" | "cancelled";

/** A download job; `received`/`total` are aggregated over all of its files. */
export interface DownloadProgress {
  id: string;
  model_id: string;
  /** File currently being transferred. */
  file: string;
  received: number;
  total: number;
  status: DownloadStatus;
  error?: string;
  provider: string;
  model_ref: string;
  variant: string;
  files_total: number;
  files_done: number;
  speed_bps: number;
  dest: string;
}

// ---------- Model hub (docs/architecture/model-hub.md §3) ----------
export interface ModelProviderInfo {
  id: string;
  name: string;
  description: string;
  homepage: string;
  /** search | files | auth | multi_file */
  capabilities: string[];
  formats: string[];
  auth: "none" | "optional" | "required";
  auth_configured: boolean;
  endpoint: string;
}

export interface RemoteModel {
  provider: string;
  id: string;
  name: string;
  publisher: string;
  description?: string;
  formats: string[];
  tags: string[];
  capabilities: string[];
  downloads?: number;
  likes?: number;
  updated_at?: string;
  license?: string;
  gated: boolean;
  params_b?: number;
  url: string;
}

export interface SearchPage {
  items: RemoteModel[];
  next_cursor?: string;
}

export interface RemoteFile {
  path: string;
  size_bytes: number;
  sha256?: string;
  role: "weights" | "mmproj" | "config" | "tokenizer" | "other";
}

export interface ModelVariant {
  id: string;
  label: string;
  format: string;
  quant?: string;
  files: RemoteFile[];
  size_bytes: number;
  fit: Fit;
  fit_detail: string;
  recommended: boolean;
  runtimes: RuntimeCompat[];
  installed: boolean;
}

export interface RemoteModelDetail {
  model: RemoteModel;
  revision: string;
  variants: ModelVariant[];
}

/**
 * Body of POST /api/models/pull: the first shape that matches wins on the server. In the hub form
 * `variant` may be left out or empty: the server then picks the only or the recommended variant.
 */
export type PullRequest =
  | { provider: string; model: string; variant?: string; revision?: string }
  | { id: string }
  | { repo: string; file: string }
  | { url: string; file_name?: string; sha256?: string };

export interface ModelSearchParams {
  provider: string;
  q: string;
  format?: string;
  sort?: "downloads" | "likes" | "updated";
  limit?: number;
  cursor?: string;
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
  /** JSON Schema of the final answer (subset: type, properties, required, items, enum, additionalProperties, minItems, maxItems). */
  output_schema?: unknown;
  /** Limits of one run (sub-agents included); a missing value falls back to the `agents.budget.*` settings. */
  budget?: BudgetLimits;
}

/** Each limit is optional; a set one is at least 1 (the backend refuses 0). */
export interface BudgetLimits {
  max_tokens?: number;
  max_turns?: number;
  max_seconds?: number;
}

// ---------- Teams ----------
export type TeamStrategy = "supervisor" | "parallel" | "pipeline";

export interface TeamMember {
  /** A local agent id, or `remote:<id>` for a remote agent. */
  agent: string;
  /** What the member is for (the description the supervisor sees). */
  role: string;
}

/** An agent that runs elsewhere and speaks A2A. Teams use it as `remote:<id>`; the token is always `SECRET_MASK`. */
export interface RemoteAgent {
  id: string;
  name: string;
  /** A2A base URL or the URL of its Agent Card. */
  url: string;
  token: string;
  /** The remote agent to ask for (`metadata.agent`); empty: its default. */
  skill?: string;
  description: string;
  /** A paired device: offered automatically, managed from the devices page. */
  paired?: boolean;
}

/** A group of agents used in chat like one agent: the session's agent id is `team:<id>`. */
export interface TeamDef {
  id: string;
  name: string;
  description: string;
  strategy: TeamStrategy;
  /** Agent that orchestrates the members; required for `supervisor`. */
  supervisor?: string;
  members: TeamMember[];
  /** Agent that merges the member results of a `parallel` team (optional). */
  aggregator?: string;
  budget?: BudgetLimits;
  /** JSON Schema of the final answer (same subset as the agents'). */
  output_schema?: unknown;
}

/** Prefix of a session's agent id that names a team. */
export const TEAM_PREFIX = "team:";
/** Prefix of the id of a remote agent in a team. */
export const REMOTE_PREFIX = "remote:";

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

/** What a rewind of a run would do (paths relative to the workspace of the run). */
export interface CheckpointInfo {
  run_id: string;
  root?: string;
  /** Put back to their original content. */
  restore: string[];
  /** Created by the run: deleted. */
  delete: string[];
  /** Outside the workspace: listed, never touched. */
  external: string[];
}

export interface RewindResult {
  run_id: string;
  restored: string[];
  deleted: string[];
  external: string[];
  failed: string[];
}

export type RunStatus = "running" | "waiting_approval" | "succeeded" | "failed" | "cancelled";

export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
}

/**
 * `name` of the stored `system` message that records how a run ended in error (`content` is the text the
 * chat showed as RUN_ERROR). Part of the history, never of the model's context (api.rs `RUN_ERROR_MESSAGE`).
 */
export const RUN_ERROR_MESSAGE = "aiw.run_error";

export interface ChatMessage {
  id: string;
  /** A `system` message named `RUN_ERROR_MESSAGE` is the error that ended a run. */
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
  | { type: "SUBAGENT_STARTED"; subagentId: string; name: string; /** Run of the sub-agent: its events are at `runEvents(runId)`. */ runId?: string }
  | { type: "SUBAGENT_FINISHED"; subagentId: string; result?: string }
  | { type: "CUSTOM"; name: string; value: unknown }
);

export interface RunState {
  todos?: { content: string; status: "pending" | "in_progress" | "completed" }[];
  usage?: { prompt_tokens: number; completion_tokens: number; tokens_per_s?: number; context_used?: number; context_max?: number };
  runtime?: string;
  model?: string;
  /** State shared by the agents of a team run (the `team_state` tool). */
  shared?: Record<string, unknown>;
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

/** Options of the executable steps; the defaults (0, 2 s, none, `fail`) are left out. */
export interface StepOptions {
  /** Extra attempts after a failure, 0..5. */
  retry?: number;
  /** Seconds between attempts, 0..300 (default 2). */
  retry_delay_s?: number;
  /** Time limit of each attempt in seconds, 1..86400. */
  timeout_s?: number;
  /** `continue`: the step stays failed but the run goes on; `{{steps.<id>.error}}` holds the error. */
  on_error?: "fail" | "continue";
}

export type WorkflowStep =
  | ({ id: string; kind: "tool"; tool: string; args: Record<string, unknown> } & StepOptions)
  | ({ id: string; kind: "agent"; agent: string; prompt: string } & StepOptions)
  | ({ id: string; kind: "llm"; profile: string; prompt: string } & StepOptions)
  /** `do` is one inline tool/agent/llm step that can use `{{item}}`, `{{item.field}}` and `{{index}}`; the output is the JSON array of its outputs. */
  | ({ id: string; kind: "foreach"; items: string; do: WorkflowStep; parallel?: number } & StepOptions)
  /** Inline tool/agent/llm steps that run together; the output is the JSON object `{inner_id: output}`. */
  | ({ id: string; kind: "parallel"; steps: WorkflowStep[] } & StepOptions)
  /** Starts another workflow (it can be disabled) and waits for it; the output is the one of its last step. */
  | ({ id: string; kind: "workflow"; workflow: string; inputs?: Record<string, unknown> } & StepOptions)
  | { id: string; kind: "approval"; message: string; timeout_s?: number; default?: "deny" | "allow" }
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
  /** The run of the workflow that started this one as a sub-workflow. */
  parent_run_id?: string;
  steps: { id: string; status: "pending" | "running" | "done" | "failed" | "skipped" | "waiting"; output?: string; error?: string; attempts?: number }[];
}

// ---------- Tools & MCP ----------
export type ToolRisk = "read" | "write" | "exec" | "network" | "computer";

export interface ToolInfo {
  name: string;
  description: string;
  /**
   * `internal` tools (todo, agent, skill, render_ui, code) are implemented by the agent loop itself: always on
   * (`enabled` and `available` are true, risk `read`), an agent profile can list them, but they have no switch
   * (`PUT /api/tools/{name}` answers 400).
   */
  source: "builtin" | "mcp" | "internal";
  server?: string;
  risk: ToolRisk;
  enabled: boolean;
  available: boolean;
  unavailable_reason?: string;
  parameters: unknown;
}

/**
 * What the API shows instead of a secret (MCP headers and environment, secret runtime settings). Sending it back
 * means "keep the stored value" (api.rs `SECRET_MASK`).
 */
export const SECRET_MASK = "••••••";

export interface McpServer {
  id: string;
  name: string;
  transport: "stdio" | "http";
  command?: string;
  args?: string[];
  url?: string;
  /**
   * Environment of a stdio server. Values are strings in practice (the Rust side holds JSON values); the ones whose
   * name looks like a secret come back as `SECRET_MASK`.
   */
  env?: Record<string, unknown>;
  /** Extra HTTP headers of an http server (e.g. `Authorization`); secret values come back as `SECRET_MASK`. */
  headers?: Record<string, unknown>;
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

// ---------- Policy & hooks (GET/PUT /api/policy, /api/hooks) ----------
export type PolicyAction = "allow" | "ask" | "deny";

export interface PolicyRule {
  /** Glob on the tool name (`shell`, `mcp__github__*`). */
  tool: string;
  /** Glob on the tool subject (path, command, url); empty = any. */
  pattern?: string | null;
  action: PolicyAction;
}

export type HookEvent = "SessionStart" | "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "Stop" | "SubagentStart" | "SubagentStop" | "PreCompact";

export interface HookConfig {
  event: HookEvent;
  /** Glob on the tool name (tool events) or on the agent id (subagent events). */
  matcher?: string | null;
  command: string;
  timeout_s: number;
}

/** Payload of the `CUSTOM` event named `aiw.image` (screenshots of browser and computer-use tools). */
export interface AiwImage {
  toolCallId: string;
  mime: string;
  /** Base64 data. */
  data: string;
}
