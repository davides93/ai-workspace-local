import type { Transport } from "./transport";
import type {
  AgEvent, AgentProfile, ApprovalRequest, AuditEntry, CatalogModel, ChatMessage, ConfigView,
  DownloadProgress, InferenceProfile, LocalModel, McpServer, RuntimeInfo, Session, SystemInfo,
  ToolInfo, WorkflowDef, WorkflowRun, WorktreeStatus,
} from "./types";

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

/** REST + SSE carrier for `aiw serve` (same routes the Tauri shell dispatches in-process). */
export class HttpTransport implements Transport {
  readonly kind: "http" | "tauri" = "http";

  constructor(protected base: string, protected token: string | null) {}

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
  runtimeAction = (id: string, action: "start" | "stop" | "refresh", opts?: { model?: string }) =>
    this.req<RuntimeInfo>("POST", `/runtimes/${encodeURIComponent(id)}/${action}`, opts ?? {});
  updateRuntimeSettings = (id: string, settings: Record<string, unknown>) =>
    this.req<RuntimeInfo>("PUT", `/runtimes/${encodeURIComponent(id)}/settings`, settings);

  runtimeLogs = (id: string) => this.req<string[]>("GET", `/runtimes/${encodeURIComponent(id)}/logs`);
  models = () => this.req<LocalModel[]>("GET", "/models");
  catalog = () => this.req<CatalogModel[]>("GET", "/models/catalog");
  pullModel = (catalogId: string) => this.req<DownloadProgress>("POST", "/models/pull", { id: catalogId });
  downloads = () => this.req<DownloadProgress[]>("GET", "/models/downloads");

  profiles = () => this.req<InferenceProfile[]>("GET", "/profiles");
  saveProfile = (p: InferenceProfile) => this.req<InferenceProfile>("PUT", `/profiles/${encodeURIComponent(p.id)}`, p);
  agents = () => this.req<AgentProfile[]>("GET", "/agents");
  saveAgent = (a: AgentProfile) => this.req<AgentProfile>("PUT", `/agents/${encodeURIComponent(a.id)}`, a);

  sessions = () => this.req<Session[]>("GET", "/sessions");
  createSession = (input: { agent_id: string; title?: string; isolation?: "none" | "worktree" }) => this.req<Session>("POST", "/sessions", input);
  deleteSession = (id: string) => this.req<void>("DELETE", `/sessions/${encodeURIComponent(id)}`);
  worktreeStatus = (sessionId: string) => this.req<WorktreeStatus>("GET", `/sessions/${encodeURIComponent(sessionId)}/worktree`);
  worktreeApply = (sessionId: string) => this.req<WorktreeStatus>("POST", `/sessions/${encodeURIComponent(sessionId)}/worktree/apply`);
  worktreeDiscard = (sessionId: string) => this.req<void>("DELETE", `/sessions/${encodeURIComponent(sessionId)}/worktree`);
  messages = (sessionId: string) => this.req<ChatMessage[]>("GET", `/sessions/${encodeURIComponent(sessionId)}/messages`);
  startRun = (sessionId: string, input: string) =>
    this.req<{ run_id: string }>("POST", `/sessions/${encodeURIComponent(sessionId)}/runs`, { input });
  cancelRun = (runId: string) => this.req<void>("POST", `/runs/${encodeURIComponent(runId)}/cancel`);
  activeRun = async (sessionId: string) => (await this.req<{ run_id: string | null }>("GET", `/sessions/${encodeURIComponent(sessionId)}/active-run`)).run_id;
  approvals = () => this.req<ApprovalRequest[]>("GET", "/approvals");
  respondApproval = (id: string, decision: "allow" | "deny", remember?: boolean) =>
    this.req<void>("POST", `/approvals/${encodeURIComponent(id)}`, { decision, remember: !!remember });

  subscribeRun(runId: string, onEvent: (e: AgEvent) => void): () => void {
    const q = this.token ? `?token=${encodeURIComponent(this.token)}` : "";
    const es = new EventSource(`${this.base}/api/runs/${encodeURIComponent(runId)}/events${q}`);
    es.onmessage = (m) => {
      try {
        const ev = JSON.parse(m.data) as AgEvent;
        onEvent(ev);
        if (ev.type === "RUN_FINISHED" || ev.type === "RUN_ERROR") es.close();
      } catch {
        /* ignore keep-alive frames */
      }
    };
    es.onerror = () => {
      // The server closes the stream after RUN_FINISHED; EventSource would reconnect forever.
      if (es.readyState === EventSource.CLOSED) return;
    };
    return () => es.close();
  }

  workflows = () => this.req<WorkflowDef[]>("GET", "/workflows");
  saveWorkflow = (w: WorkflowDef) => this.req<WorkflowDef>("PUT", `/workflows/${encodeURIComponent(w.id)}`, w);
  runWorkflow = (id: string, inputs?: Record<string, unknown>) =>
    this.req<WorkflowRun>("POST", `/workflows/${encodeURIComponent(id)}/run`, { inputs: inputs ?? {} });
  workflowRuns = () => this.req<WorkflowRun[]>("GET", "/workflows/runs");

  tools = () => this.req<ToolInfo[]>("GET", "/tools");
  setToolEnabled = (name: string, enabled: boolean) =>
    this.req<void>("PUT", `/tools/${encodeURIComponent(name)}`, { enabled });
  mcpServers = () => this.req<McpServer[]>("GET", "/mcp/servers");
  saveMcpServer = (s: Partial<McpServer> & { name: string; transport: "stdio" | "http" }) =>
    this.req<McpServer>("POST", "/mcp/servers", s);
  removeMcpServer = (id: string) => this.req<void>("DELETE", `/mcp/servers/${encodeURIComponent(id)}`);

  config = () => this.req<ConfigView>("GET", "/config");
  setConfig = (layer: "global" | "device" | "workspace", key: string, value: unknown) =>
    this.req<ConfigView>("PUT", "/config", { layer, key, value });
  audit = () => this.req<AuditEntry[]>("GET", "/audit");

  /** Exchanges the 6-digit pairing code shown on the host device for the API token. */
  async pair(code: string): Promise<void> {
    const r = await this.req<{ token: string }>("POST", "/pair", { code });
    this.token = r.token;
    try {
      localStorage.setItem("aiw.token", r.token);
    } catch {
      /* storage unavailable: token kept in memory */
    }
  }
}
