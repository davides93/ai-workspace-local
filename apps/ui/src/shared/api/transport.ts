import { safeStorage, serverOrigin, storedToken, storeToken } from "./tokens";
import type {
  AgEvent, AgentProfile, AuditEntry, CatalogModel, ChatMessage, ConfigView, DownloadProgress,
  InferenceProfile, LocalModel, McpServer, RuntimeInfo, Session, SystemInfo, ToolInfo,
  WorkflowDef, WorkflowRun, ApprovalRequest, WorktreeStatus, MeshState, PeerDevice,
  ModelProviderInfo, ModelSearchParams, PullRequest, RemoteModelDetail, SearchPage,
  PolicyRule, HookConfig, VoiceStatus, Transcript,
} from "./types";

/** What the current carrier can do; the UI branches on this instead of sniffing `kind`. */
export interface TransportCapabilities {
  /** Sample data, no real backend: the shell shows the demo banner. */
  demo: boolean;
  /** Running inside the Tauri shell (native dialogs, notifications). */
  native: boolean;
  /** This carrier exchanges a pairing code for an API token (browser on another device). */
  pairing: boolean;
}

/**
 * One API, three carriers (ADR 0002):
 *  - TauriTransport: in-app IPC (desktop + mobile shells)
 *  - HttpTransport:  REST + SSE against `aiw serve` (browser, LAN devices, dev server)
 *  - MockTransport:  scripted demo data (interactive mockup, offline previews)
 */
export interface Transport {
  readonly kind: "tauri" | "http" | "mock";
  readonly capabilities: TransportCapabilities;
  /** Shown in the demo banner (e.g. why the app fell back to the demo). */
  readonly notice?: string;
  /** Exchanges the 6-digit pairing code shown on the host device for the API token. */
  pair?(code: string): Promise<void>;

  system(): Promise<SystemInfo>;
  kill(): Promise<void>;

  runtimes(): Promise<RuntimeInfo[]>;
  /** Re-runs detection of every runtime (POST /api/runtimes/refresh). */
  refreshRuntimes(): Promise<RuntimeInfo[]>;
  runtimeAction(id: string, action: "start" | "stop" | "refresh", opts?: { model?: string }): Promise<RuntimeInfo>;
  updateRuntimeSettings(id: string, settings: Record<string, unknown>): Promise<RuntimeInfo>;
  runtimeLogs(id: string): Promise<string[]>;

  models(): Promise<LocalModel[]>;
  catalog(): Promise<CatalogModel[]>;
  modelProviders(): Promise<ModelProviderInfo[]>;
  /**
   * `token: null` removes the stored token, `undefined` leaves it as it is; the token never comes back
   * from the server. `endpoint` omitted leaves it as it is, `""` restores the provider's default.
   */
  setProviderAuth(id: string, token: string | null | undefined, endpoint?: string): Promise<ModelProviderInfo>;
  searchModels(p: ModelSearchParams): Promise<SearchPage>;
  remoteModel(provider: string, model: string, revision?: string): Promise<RemoteModelDetail>;
  /** A string is the id of a curated catalog entry. */
  pullModel(req: string | PullRequest): Promise<DownloadProgress>;
  downloads(): Promise<DownloadProgress[]>;
  cancelDownload(id: string): Promise<DownloadProgress>;
  /** Removes a finished job from the list. */
  clearDownload(id: string): Promise<void>;
  deleteModel(id: string): Promise<void>;

  profiles(): Promise<InferenceProfile[]>;
  saveProfile(p: InferenceProfile): Promise<InferenceProfile>;
  deleteProfile(id: string): Promise<void>;
  agents(): Promise<AgentProfile[]>;
  saveAgent(a: AgentProfile): Promise<AgentProfile>;
  deleteAgent(id: string): Promise<void>;

  sessions(): Promise<Session[]>;
  getSession(id: string): Promise<Session>;
  createSession(input: { agent_id: string; title?: string; isolation?: "none" | "worktree" }): Promise<Session>;
  renameSession(id: string, title: string): Promise<Session>;
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
  deleteWorkflow(id: string): Promise<void>;
  runWorkflow(id: string, inputs?: Record<string, unknown>): Promise<WorkflowRun>;
  workflowRuns(): Promise<WorkflowRun[]>;

  tools(): Promise<ToolInfo[]>;
  setToolEnabled(name: string, enabled: boolean): Promise<void>;
  mcpServers(): Promise<McpServer[]>;
  saveMcpServer(s: Partial<McpServer> & { name: string; transport: "stdio" | "http" }): Promise<McpServer>;
  removeMcpServer(id: string): Promise<void>;

  policy(): Promise<{ rules: PolicyRule[] }>;
  setPolicy(rules: PolicyRule[]): Promise<{ rules: PolicyRule[] }>;
  hooks(): Promise<{ hooks: HookConfig[] }>;
  setHooks(hooks: HookConfig[]): Promise<{ hooks: HookConfig[] }>;

  mesh(): Promise<MeshState>;
  pairPeer(address: string, code: string): Promise<PeerDevice>;
  removePeer(id: string): Promise<void>;
  /** Link that pairs a phone in one step (its camera opens the web UI) and its QR as SVG. */
  pairingQr(): Promise<{ url: string; svg: string }>;

  /** Voice mode: the speech-to-text backend that would be used, and the voice settings. */
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
  // The API token may arrive in the URL (…/?token=…): keep it in storage and drop it from the
  // address bar so it does not end up in the history, in screenshots or in shared links.
  const server = params.get("server") ?? "";
  const origin = serverOrigin(server, location.href);
  const urlToken = params.get("token");
  if (urlToken) {
    storeToken(safeStorage(), origin, urlToken);
    params.delete("token");
  }
  // A stored token only goes back to the origin that issued it (never to a `?server=` elsewhere).
  const http = new HttpTransport(server, urlToken ?? storedToken(safeStorage(), origin, location.origin), origin);
  // Opened from the pairing QR (…/?pair=123456): pair at once, then drop the code from the URL.
  const pairCode = params.get("pair");
  if (pairCode && /^\d{6}$/.test(pairCode)) {
    try {
      await http.pair(pairCode);
    } catch {
      /* expired code: the pairing screen asks for a new one */
    }
    params.delete("pair");
  }
  if (urlToken || pairCode) {
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
