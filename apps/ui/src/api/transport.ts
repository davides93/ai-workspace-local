import type {
  AgEvent, AgentProfile, AuditEntry, CatalogModel, ChatMessage, ConfigView, DownloadProgress,
  InferenceProfile, LocalModel, McpServer, RuntimeInfo, Session, SystemInfo, ToolInfo,
  WorkflowDef, WorkflowRun, ApprovalRequest, WorktreeStatus, MeshState, PeerDevice, VoiceStatus, Transcript,
} from "./types";

/**
 * One API, three carriers (ADR 0002):
 *  - TauriTransport: in-app IPC (desktop + mobile shells)
 *  - HttpTransport:  REST + SSE against `aiw serve` (browser, LAN devices, dev server)
 *  - MockTransport:  scripted demo data (interactive mockup, offline previews)
 */
export interface Transport {
  readonly kind: "tauri" | "http" | "mock";

  system(): Promise<SystemInfo>;
  kill(): Promise<void>;

  runtimes(): Promise<RuntimeInfo[]>;
  runtimeAction(id: string, action: "start" | "stop" | "refresh", opts?: { model?: string }): Promise<RuntimeInfo>;
  updateRuntimeSettings(id: string, settings: Record<string, unknown>): Promise<RuntimeInfo>;
  runtimeLogs(id: string): Promise<string[]>;

  models(): Promise<LocalModel[]>;
  catalog(): Promise<CatalogModel[]>;
  pullModel(catalogId: string): Promise<DownloadProgress>;
  downloads(): Promise<DownloadProgress[]>;

  profiles(): Promise<InferenceProfile[]>;
  saveProfile(p: InferenceProfile): Promise<InferenceProfile>;
  agents(): Promise<AgentProfile[]>;
  saveAgent(a: AgentProfile): Promise<AgentProfile>;

  sessions(): Promise<Session[]>;
  createSession(input: { agent_id: string; title?: string; isolation?: "none" | "worktree" }): Promise<Session>;
  deleteSession(id: string): Promise<void>;
  worktreeStatus(sessionId: string): Promise<WorktreeStatus>;
  worktreeApply(sessionId: string): Promise<WorktreeStatus>;
  worktreeDiscard(sessionId: string): Promise<void>;
  messages(sessionId: string): Promise<ChatMessage[]>;
  /** `kind: "a2ui.action"` when an A2UI surface reports a user action. */
  startRun(sessionId: string, input: string, kind?: "a2ui.action"): Promise<{ run_id: string }>;
  cancelRun(runId: string): Promise<void>;
  subscribeRun(runId: string, onEvent: (e: AgEvent) => void): () => void;
  activeRun(sessionId: string): Promise<string | null>;
  approvals(): Promise<ApprovalRequest[]>;
  respondApproval(id: string, decision: "allow" | "deny", remember?: boolean): Promise<void>;

  workflows(): Promise<WorkflowDef[]>;
  saveWorkflow(w: WorkflowDef): Promise<WorkflowDef>;
  runWorkflow(id: string, inputs?: Record<string, unknown>): Promise<WorkflowRun>;
  workflowRuns(): Promise<WorkflowRun[]>;

  tools(): Promise<ToolInfo[]>;
  setToolEnabled(name: string, enabled: boolean): Promise<void>;
  mcpServers(): Promise<McpServer[]>;
  saveMcpServer(s: Partial<McpServer> & { name: string; transport: "stdio" | "http" }): Promise<McpServer>;
  removeMcpServer(id: string): Promise<void>;

  mesh(): Promise<MeshState>;
  pairPeer(address: string, code: string): Promise<PeerDevice>;
  removePeer(id: string): Promise<void>;
  /** Link that pairs a phone in one step (its camera opens the web UI) and its QR as SVG. */
  pairingQr(): Promise<{ url: string; svg: string }>;

  voice(): Promise<VoiceStatus>;
  /** Speech to text of a 16 kHz mono WAV recorded by the app; `language` overrides the setting. */
  transcribe(wav: Blob, language?: string): Promise<Transcript>;

  config(): Promise<ConfigView>;
  setConfig(layer: "global" | "device" | "workspace", key: string, value: unknown): Promise<ConfigView>;
  audit(): Promise<AuditEntry[]>;
}

declare const __AIW_FORCE_MOCK__: boolean;

/** Picks the carrier for the current environment. */
export async function createTransport(): Promise<Transport> {
  const params = new URLSearchParams(location.search);
  const forceMock = typeof __AIW_FORCE_MOCK__ !== "undefined" && __AIW_FORCE_MOCK__;
  if (forceMock || params.get("demo") === "1") {
    const { MockTransport } = await import("./mock");
    return new MockTransport();
  }
  if ((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) {
    const { TauriTransport } = await import("./tauri");
    return new TauriTransport();
  }
  const { HttpTransport } = await import("./http");
  const http = new HttpTransport(params.get("server") ?? "", params.get("token") ?? localStorageGet("aiw.token"));
  // Opened from the pairing QR (…/?pair=123456): pair at once, then drop the code from the URL.
  const pairCode = params.get("pair");
  if (pairCode && /^\d{6}$/.test(pairCode)) {
    try {
      await http.pair(pairCode);
    } catch {
      /* expired code: the pairing screen asks for a new one */
    }
    params.delete("pair");
    const q = params.toString();
    history.replaceState(null, "", `${location.pathname}${q ? `?${q}` : ""}${location.hash}`);
  }
  try {
    await http.system();
    return http;
  } catch (e) {
    // 401 = we are on another device of the LAN: the app shows the pairing screen.
    if ((e as { status?: number }).status === 401) return http;
    // No backend reachable: fall back to the demo so the UI is still explorable.
    const { MockTransport } = await import("./mock");
    return new MockTransport("Backend non raggiungibile: modalità demo");
  }
}

function localStorageGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
