import { safeStorage, serverOrigin, storeToken } from "./tokens";
import type { Transport, TransportCapabilities } from "./transport";
import type {
  AgEvent, AgentProfile, RoutingStats, TeamDef, ApprovalRequest, AuditEntry, CatalogModel, ChatMessage, ConfigView, EvalComparison, EvalReport, EvalRunRequest, EvalSuite, Recommendations, Feedback, Lesson, LessonStatus, Memory, ProactiveStatus, Proposal, ProposalQuery, TickResult, MemoryCreate, MemoryPatch, MemoryQuery, OptimizeRequest, PromptOptimization,
  DownloadProgress, InferenceProfile, LocalModel, McpServer, RuntimeInfo, Session, SystemInfo,
  ToolInfo, WorkflowDef, WorkflowRun, WorktreeStatus, CheckpointInfo, RewindResult, MeshState, PeerDevice, RemoteAgent,
  AdapterInfo, DatasetInfo, DatasetSpec, TrainDoctor, TrainJob, TrainJobRequest,
  ModelProviderInfo, ModelSearchParams, PullRequest, RemoteModelDetail, SearchPage,
  PolicyRule, HookConfig, VoiceStatus, Transcript, InstallJob, InstallPlan, UpdateStatus, ThinkingMode,
} from "./types";

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

const enc = encodeURIComponent;

/** Builds `?a=1&b=2`, skipping undefined and empty values. */
export function queryString(params: Record<string, string | number | undefined | null>): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== "") q.set(k, String(v));
  }
  const s = q.toString();
  return s ? `?${s}` : "";
}

/** Body of POST /api/models/pull for the string form (id of a curated catalog entry). */
export function pullBody(req: string | PullRequest): PullRequest {
  return typeof req === "string" ? { id: req } : req;
}

/** Body of PUT /api/models/providers/{id}/auth: an absent field is left unchanged, `token: null` removes the token. */
export function authBody(token: string | null | undefined, endpoint?: string): { token?: string | null; endpoint?: string } {
  const body: { token?: string | null; endpoint?: string } = {};
  if (token !== undefined) body.token = token;
  if (endpoint !== undefined) body.endpoint = endpoint;
  return body;
}

/** REST + SSE carrier for `aiw serve` (same routes the Tauri shell dispatches in-process). */
export class HttpTransport implements Transport {
  readonly kind: "http" | "tauri" = "http";
  readonly capabilities: TransportCapabilities = { demo: false, native: false, pairing: true };

  /** `origin` = origin of `base`, used to store the token obtained by pairing for this server only. */
  constructor(protected base: string, protected token: string | null, protected origin: string | null = null) {}

  protected async req<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { Accept: "application/json" };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    const res = await fetch(`${this.base}/api${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.text();
    if (!res.ok) {
      let msg = text;
      try {
        msg = JSON.parse(text).error ?? text;
      } catch {
        /* plain text error */
      }
      throw new ApiError(res.status, msg || res.statusText);
    }
    return (text ? JSON.parse(text) : undefined) as T;
  }

  system = () => this.req<SystemInfo>("GET", "/system");
  kill = () => this.req<void>("POST", "/kill");

  runtimes = () => this.req<RuntimeInfo[]>("GET", "/runtimes");
  refreshRuntimes = () => this.req<RuntimeInfo[]>("POST", "/runtimes/refresh");
  runtimeAction = (id: string, action: "start" | "stop" | "refresh", opts?: { model?: string }) =>
    this.req<RuntimeInfo>("POST", `/runtimes/${enc(id)}/${action}`, opts ?? {});
  updateRuntimeSettings = (id: string, settings: Record<string, unknown>) =>
    this.req<RuntimeInfo>("PUT", `/runtimes/${enc(id)}/settings`, settings);
  runtimeLogs = (id: string) => this.req<string[]>("GET", `/runtimes/${enc(id)}/logs`);
  runtimeInstallPlan = (id: string) => this.req<InstallPlan>("GET", `/runtimes/${enc(id)}/install-plan`);
  previewSystemModel = (id: string, prompt: string) => this.req<string>("POST", "/model-preview", { id, prompt });
  installRuntime = (id: string, start = false) => this.req<InstallJob>("POST", `/runtimes/${enc(id)}/install${start ? "?start=true" : ""}`, {});
  runtimeInstall = (id: string) => this.req<InstallJob | null>("GET", `/runtimes/${enc(id)}/install`);

  updateStatus = () => this.req<UpdateStatus>("GET", "/update");
  checkUpdate = () => this.req<UpdateStatus>("POST", "/update/check", {});
  downloadUpdate = () => this.req<UpdateStatus>("POST", "/update/download", {});
  installUpdate = () => this.req<UpdateStatus>("POST", "/update/install", {});

  models = () => this.req<LocalModel[]>("GET", "/models");
  catalog = () => this.req<CatalogModel[]>("GET", "/models/catalog");
  recommend = () => this.req<Recommendations>("GET", "/recommend");
  modelProviders = () => this.req<ModelProviderInfo[]>("GET", "/models/providers");
  setProviderAuth = (id: string, token: string | null | undefined, endpoint?: string) =>
    this.req<ModelProviderInfo>("PUT", `/models/providers/${enc(id)}/auth`, authBody(token, endpoint));
  searchModels = (p: ModelSearchParams) =>
    this.req<SearchPage>("GET", `/models/search${queryString({ provider: p.provider, q: p.q, format: p.format, sort: p.sort, limit: p.limit, cursor: p.cursor })}`);
  remoteModel = (provider: string, model: string, revision?: string) =>
    this.req<RemoteModelDetail>("GET", `/models/remote${queryString({ provider, model, revision })}`);
  pullModel = (req: string | PullRequest) => this.req<DownloadProgress>("POST", "/models/pull", pullBody(req));
  downloads = () => this.req<DownloadProgress[]>("GET", "/models/downloads");
  cancelDownload = (id: string) => this.req<DownloadProgress>("POST", `/models/downloads/${enc(id)}/cancel`);
  clearDownload = (id: string) => this.req<void>("DELETE", `/models/downloads/${enc(id)}`);
  deleteModel = (id: string) => this.req<void>("DELETE", `/models/${enc(id)}`);

  profiles = () => this.req<InferenceProfile[]>("GET", "/profiles");
  saveProfile = (p: InferenceProfile) => this.req<InferenceProfile>("PUT", `/profiles/${enc(p.id)}`, p);
  deleteProfile = (id: string) => this.req<void>("DELETE", `/profiles/${enc(id)}`);
  agents = () => this.req<AgentProfile[]>("GET", "/agents");
  saveAgent = (a: AgentProfile) => this.req<AgentProfile>("PUT", `/agents/${enc(a.id)}`, a);
  deleteAgent = (id: string) => this.req<void>("DELETE", `/agents/${enc(id)}`);
  agentRouting = (id: string) => this.req<RoutingStats>("GET", `/agents/${enc(id)}/routing`);
  resetAgentRouting = (id: string) => this.req<void>("DELETE", `/agents/${enc(id)}/routing`);
  remoteAgents = () => this.req<RemoteAgent[]>("GET", "/remote-agents");
  saveRemoteAgent = (r: RemoteAgent) => this.req<RemoteAgent>("PUT", `/remote-agents/${enc(r.id)}`, r);
  deleteRemoteAgent = (id: string) => this.req<void>("DELETE", `/remote-agents/${enc(id)}`);
  teams = () => this.req<TeamDef[]>("GET", "/teams");
  saveTeam = (t: TeamDef) => this.req<TeamDef>("PUT", `/teams/${enc(t.id)}`, t);
  deleteTeam = (id: string) => this.req<void>("DELETE", `/teams/${enc(id)}`);

  sessions = () => this.req<Session[]>("GET", "/sessions");
  getSession = (id: string) => this.req<Session>("GET", `/sessions/${enc(id)}`);
  createSession = (input: { agent_id: string; title?: string; isolation?: "none" | "worktree" }) => this.req<Session>("POST", "/sessions", input);
  renameSession = (id: string, title: string) => this.req<Session>("PATCH", `/sessions/${enc(id)}`, { title });
  deleteSession = (id: string) => this.req<void>("DELETE", `/sessions/${enc(id)}`);
  mesh = () => this.req<MeshState>("GET", "/mesh");
  pairPeer = (address: string, code: string) => this.req<PeerDevice>("POST", "/mesh/pair", { address, code });
  removePeer = (id: string) => this.req<void>("DELETE", `/mesh/peers/${enc(id)}`);
  pairingQr = () => this.req<{ url: string; svg: string }>("GET", "/pairing/qr");

  voice = () => this.req<VoiceStatus>("GET", "/voice");

  /** Raw audio body (no JSON/base64 overhead). The Tauri carrier overrides it. */
  async transcribe(wav: Blob, language?: string): Promise<Transcript> {
    const headers: Record<string, string> = { Accept: "application/json", "Content-Type": wav.type || "audio/wav" };
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    const res = await fetch(`${this.base}/api/voice/transcribe${queryString({ language: language !== "auto" ? language : undefined })}`, {
      method: "POST",
      headers,
      body: wav,
    });
    const text = await res.text();
    if (!res.ok) {
      let msg = text;
      try {
        msg = JSON.parse(text).error ?? text;
      } catch {
        /* plain text error */
      }
      throw new ApiError(res.status, msg || res.statusText);
    }
    return JSON.parse(text) as Transcript;
  }

  worktreeStatus = (sessionId: string) => this.req<WorktreeStatus>("GET", `/sessions/${enc(sessionId)}/worktree`);
  worktreeApply = (sessionId: string) => this.req<WorktreeStatus>("POST", `/sessions/${enc(sessionId)}/worktree/apply`);
  worktreeDiscard = (sessionId: string) => this.req<void>("DELETE", `/sessions/${enc(sessionId)}/worktree`);
  messages = (sessionId: string) => this.req<ChatMessage[]>("GET", `/sessions/${enc(sessionId)}/messages`);
  sessionFeedback = (sessionId: string) => this.req<Feedback[]>("GET", `/sessions/${enc(sessionId)}/feedback`);
  setFeedback = (sessionId: string, messageId: string, rating: 1 | -1, note?: string) =>
    this.req<Feedback>("PUT", `/sessions/${enc(sessionId)}/messages/${enc(messageId)}/feedback`, note ? { rating, note } : { rating });
  deleteFeedback = (sessionId: string, messageId: string) =>
    this.req<void>("DELETE", `/sessions/${enc(sessionId)}/messages/${enc(messageId)}/feedback`);
  startRun = (sessionId: string, input: string, kind?: "a2ui.action", thinking?: ThinkingMode) =>
    this.req<{ run_id: string }>("POST", `/sessions/${enc(sessionId)}/runs`, { input, ...(kind ? { kind } : {}), ...(thinking ? { thinking } : {}) });
  sessionCheckpoints = (sessionId: string) => this.req<CheckpointInfo[]>("GET", `/sessions/${enc(sessionId)}/checkpoints`);
  rewindRun = (runId: string) => this.req<RewindResult>("POST", `/runs/${enc(runId)}/rewind`);
  cancelRun = (runId: string) => this.req<void>("POST", `/runs/${enc(runId)}/cancel`);
  activeRun = async (sessionId: string) => (await this.req<{ run_id: string | null }>("GET", `/sessions/${enc(sessionId)}/active-run`)).run_id;
  runEvents = async (runId: string) =>
    (await this.req<{ seq: number; event: AgEvent }[]>("GET", `/runs/${enc(runId)}/events.json`)).map((e) => e.event);
  approvals = () => this.req<ApprovalRequest[]>("GET", "/approvals");
  respondApproval = (id: string, decision: "allow" | "deny", remember?: boolean) =>
    this.req<void>("POST", `/approvals/${enc(id)}`, { decision, remember: !!remember });

  /**
   * Streams the AG-UI events of a run over SSE. The server replays the whole history on every
   * connection, so events whose `seq` (the SSE event id) was already delivered are dropped; on a
   * network error the stream is reopened with `?after=<last seq>` (Last-Event-ID semantics, which
   * EventSource cannot express for a manually created reconnect).
   */
  subscribeRun(runId: string, onEvent: (e: AgEvent) => void): () => void {
    let lastSeq = -1;
    let es: EventSource | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let closed = false;
    let attempt = 0;

    const connect = () => {
      // EventSource cannot set headers, so the token travels in the query (the server accepts it).
      const q = queryString({ token: this.token, after: lastSeq >= 0 ? lastSeq : undefined });
      es = new EventSource(`${this.base}/api/runs/${enc(runId)}/events${q}`);
      es.onopen = () => {
        attempt = 0;
      };
      es.onmessage = (m) => {
        const seq = m.lastEventId === "" ? NaN : Number(m.lastEventId);
        if (!Number.isNaN(seq)) {
          if (seq <= lastSeq) return;
          lastSeq = seq;
        }
        let ev: AgEvent;
        try {
          ev = JSON.parse(m.data) as AgEvent;
        } catch {
          return; // keep-alive frame
        }
        onEvent(ev);
        if (ev.type === "RUN_FINISHED" || ev.type === "RUN_ERROR") stop();
      };
      es.onerror = () => {
        // Take over from EventSource's own reconnect so the retry carries `after`.
        es?.close();
        es = null;
        if (closed) return;
        attempt++;
        timer = setTimeout(connect, Math.min(5000, 500 * 2 ** Math.min(attempt, 4)));
      };
    };

    const stop = () => {
      closed = true;
      if (timer) clearTimeout(timer);
      es?.close();
      es = null;
    };
    connect();
    return stop;
  }

  workflows = () => this.req<WorkflowDef[]>("GET", "/workflows");
  saveWorkflow = (w: WorkflowDef) => this.req<WorkflowDef>("PUT", `/workflows/${enc(w.id)}`, w);
  deleteWorkflow = (id: string) => this.req<void>("DELETE", `/workflows/${enc(id)}`);
  runWorkflow = (id: string, inputs?: Record<string, unknown>) =>
    this.req<WorkflowRun>("POST", `/workflows/${enc(id)}/run`, { inputs: inputs ?? {} });
  workflowRuns = () => this.req<WorkflowRun[]>("GET", "/workflows/runs");

  lessons = (status?: LessonStatus) => this.req<Lesson[]>("GET", status ? `/lessons?status=${enc(status)}` : "/lessons");
  approveLesson = (id: string, edit?: { title?: string; content?: string }) => this.req<Lesson>("POST", `/lessons/${enc(id)}/approve`, edit ?? {});
  rejectLesson = (id: string) => this.req<Lesson>("POST", `/lessons/${enc(id)}/reject`, {});
  reflectSession = (sessionId: string) => this.req<Lesson>("POST", `/sessions/${enc(sessionId)}/reflect`, {});
  memories = (query?: MemoryQuery) => {
    const p = new URLSearchParams();
    if (query?.status) p.set("status", query.status);
    if (query?.kind) p.set("kind", query.kind);
    if (query?.q?.trim()) p.set("q", query.q.trim());
    const qs = p.toString();
    return this.req<Memory[]>("GET", qs ? `/memories?${qs}` : "/memories");
  };
  createMemory = (body: MemoryCreate) => this.req<Memory>("POST", "/memories", body);
  updateMemory = (id: string, patch: MemoryPatch) => this.req<Memory>("PATCH", `/memories/${enc(id)}`, patch);
  forgetMemory = (id: string) => this.req<Memory>("POST", `/memories/${enc(id)}/forget`, {});
  restoreMemory = (id: string) => this.req<Memory>("POST", `/memories/${enc(id)}/restore`, {});
  approveMemory = (id: string) => this.req<Memory>("POST", `/memories/${enc(id)}/approve`, {});
  rejectMemory = (id: string) => this.req<Memory>("POST", `/memories/${enc(id)}/reject`, {});
  deleteMemory = (id: string) => this.req<void>("DELETE", `/memories/${enc(id)}`);
  proposals = (query?: ProposalQuery) => {
    const qs = new URLSearchParams();
    if (query?.status) qs.set("status", query.status);
    if (query?.kind) qs.set("kind", query.kind);
    return this.req<Proposal[]>("GET", qs.size ? `/proposals?${qs}` : "/proposals");
  };
  approveProposal = (id: string, instruction?: string) => this.req<Proposal>("POST", `/proposals/${enc(id)}/approve`, instruction ? { instruction } : {});
  dismissProposal = (id: string) => this.req<Proposal>("POST", `/proposals/${enc(id)}/dismiss`, {});
  proactiveStatus = () => this.req<ProactiveStatus>("GET", "/proactive/status");
  proactiveTick = () => this.req<TickResult>("POST", "/proactive/tick", {});
  deleteAllMemories = () => this.req<{ deleted: number }>("DELETE", "/memories?confirm=true");
  evalSuites = () => this.req<EvalSuite[]>("GET", "/evals");
  saveEvalSuite = (s: EvalSuite) => this.req<EvalSuite>("PUT", `/evals/${enc(s.id)}`, s);
  deleteEvalSuite = (id: string) => this.req<void>("DELETE", `/evals/${enc(id)}`);
  startEval = (suiteId: string, req?: EvalRunRequest) => this.req<EvalReport>("POST", `/evals/${enc(suiteId)}/runs`, req ?? {});
  evalReports = () => this.req<EvalReport[]>("GET", "/eval-runs");
  evalReport = (id: string) => this.req<EvalReport>("GET", `/eval-runs/${enc(id)}`);
  compareEvalReports = (a: string, b: string) => this.req<EvalComparison>("GET", `/eval-runs/compare?a=${enc(a)}&b=${enc(b)}`);
  startPromptOptimization = (suiteId: string, req: OptimizeRequest) => this.req<PromptOptimization>("POST", `/evals/${enc(suiteId)}/optimize`, req);
  promptOptimizations = () => this.req<PromptOptimization[]>("GET", "/prompt-optimizations");
  promptOptimization = (id: string) => this.req<PromptOptimization>("GET", `/prompt-optimizations/${enc(id)}`);
  applyPromptOptimization = (id: string, variant: number) => this.req<PromptOptimization>("POST", `/prompt-optimizations/${enc(id)}/apply`, { variant });

  trainDoctor = () => this.req<TrainDoctor>("GET", "/train/doctor");
  installTrainer = (id: string) => this.req<InstallJob>("POST", `/trainers/${enc(id)}/install`);
  trainerInstall = (id: string) => this.req<InstallJob | null>("GET", `/trainers/${enc(id)}/install`);
  datasets = () => this.req<DatasetInfo[]>("GET", "/datasets");
  importSftDataset = (name: string, jsonl: string) => this.req<DatasetInfo>("POST", "/dataset-import", { name, jsonl });
  buildDataset = (spec: DatasetSpec) => this.req<DatasetInfo>("POST", "/datasets", spec);
  deleteDataset = (id: string) => this.req<void>("DELETE", `/datasets/${enc(id)}`);
  trainJobs = () => this.req<TrainJob[]>("GET", "/train/jobs");
  startTrainJob = (req: TrainJobRequest) => this.req<TrainJob>("POST", "/train/jobs", req);
  cancelTrainJob = (id: string) => this.req<TrainJob>("POST", `/train/jobs/${enc(id)}/cancel`);
  trainJobLog = async (id: string, lines = 200) => (await this.req<{ lines: string[] }>("GET", `/train/jobs/${enc(id)}/log?lines=${lines}`)).lines;
  adapters = () => this.req<AdapterInfo[]>("GET", "/adapters");
  deleteAdapter = (id: string) => this.req<void>("DELETE", `/adapters/${enc(id)}`);

  tools = () => this.req<ToolInfo[]>("GET", "/tools");
  setToolEnabled = (name: string, enabled: boolean) => this.req<void>("PUT", `/tools/${enc(name)}`, { enabled });
  mcpServers = () => this.req<McpServer[]>("GET", "/mcp/servers");
  saveMcpServer = (s: Partial<McpServer> & { name: string; transport: "stdio" | "http" }) => this.req<McpServer>("POST", "/mcp/servers", s);
  removeMcpServer = (id: string) => this.req<void>("DELETE", `/mcp/servers/${enc(id)}`);

  policy = () => this.req<{ rules: PolicyRule[] }>("GET", "/policy");
  setPolicy = (rules: PolicyRule[]) => this.req<{ rules: PolicyRule[] }>("PUT", "/policy", { rules });
  hooks = () => this.req<{ hooks: HookConfig[] }>("GET", "/hooks");
  setHooks = (hooks: HookConfig[]) => this.req<{ hooks: HookConfig[] }>("PUT", "/hooks", { hooks });

  config = () => this.req<ConfigView>("GET", "/config");
  setConfig = (layer: "global" | "device" | "workspace", key: string, value: unknown) =>
    this.req<ConfigView>("PUT", "/config", { layer, key, value });
  audit = () => this.req<AuditEntry[]>("GET", "/audit");

  /** Exchanges the 6-digit pairing code shown on the host device for the API token. */
  async pair(code: string): Promise<void> {
    const r = await this.req<{ token: string }>("POST", "/pair", { code });
    this.token = r.token;
    storeToken(safeStorage(), this.origin ?? serverOrigin(this.base, location.href), r.token);
  }
}
