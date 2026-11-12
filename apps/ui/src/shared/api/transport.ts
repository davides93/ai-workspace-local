import { safeStorage, serverOrigin, storedToken, storeToken } from "./tokens";
import type {
  AgEvent, AgentProfile, AttachmentData, AttachmentUpload, RoutingStats, TeamDef, AuditEntry, CatalogModel, ChatMessage, ConfigView, DownloadProgress, EvalComparison, EvalReport, EvalRunRequest, EvalSuite, Recommendations, Feedback, Lesson, LessonStatus, Memory, ProactiveStatus, BackgroundRequest, BackgroundStatus, BackgroundSummary, Proposal, ProposalQuery, TickResult, MemoryCreate, MemoryPatch, MemoryQuery, OptimizeRequest, PromptOptimization,
  InferenceProfile, LocalModel, McpServer, RuntimeInfo, Session, SystemInfo, ToolInfo,
  WorkflowDef, WorkflowRun, ApprovalRequest, WorktreeStatus, CheckpointInfo, RewindResult, MeshState, PeerDevice, RemoteAgent,
  AdapterInfo, DatasetInfo, DatasetSpec, TrainDoctor, TrainJob, TrainJobRequest,
  ModelProviderInfo, ModelSearchParams, PullRequest, RemoteModelDetail, SearchPage,
  PolicyRule, HookConfig, VoiceStatus, Transcript, InstallJob, InstallPlan, UpdateStatus, ThinkingMode,
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
  /**
   * Shows the agent's native browser again, on the page it has (mobile and Mac App Store).
   * Resolves to false when this device has none.
   */
  openBrowser?(): Promise<boolean>;

  system(): Promise<SystemInfo>;
  kill(): Promise<void>;

  runtimes(): Promise<RuntimeInfo[]>;
  /** Re-runs detection of every runtime (POST /api/runtimes/refresh). */
  refreshRuntimes(): Promise<RuntimeInfo[]>;
  runtimeAction(id: string, action: "start" | "stop" | "refresh", opts?: { model?: string }): Promise<RuntimeInfo>;
  updateRuntimeSettings(id: string, settings: Record<string, unknown>): Promise<RuntimeInfo>;
  runtimeLogs(id: string): Promise<string[]>;
  /** Steps and missing prerequisites of installing a runtime. */
  runtimeInstallPlan(id: string): Promise<InstallPlan>;
  /** Starts installing a runtime in the background (no further confirmation). */
  previewSystemModel(id: string, prompt: string): Promise<string>;
  installRuntime(id: string, start?: boolean): Promise<InstallJob>;
  /** The last installation of a runtime, or null when none ran. */
  runtimeInstall(id: string): Promise<InstallJob | null>;

  /** Self-update from the GitHub releases (GET /api/update). */
  updateStatus(): Promise<UpdateStatus>;
  /** Looks for a newer release in the configured channel. */
  checkUpdate(): Promise<UpdateStatus>;
  /** Downloads the installer and verifies it against the published SHA-256 sums. */
  downloadUpdate(): Promise<UpdateStatus>;
  /** Hands the verified installer to the operating system. */
  installUpdate(): Promise<UpdateStatus>;

  models(): Promise<LocalModel[]>;
  catalog(): Promise<CatalogModel[]>;
  /** The model adviser: the best installed and the best to download model for each task. */
  recommend(): Promise<Recommendations>;
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
  /** What the bandit router learned about the candidate profiles of an agent. */
  agentRouting(id: string): Promise<RoutingStats>;
  /** Forgets it. */
  resetAgentRouting(id: string): Promise<void>;
  remoteAgents(): Promise<RemoteAgent[]>;
  /** A token equal to `SECRET_MASK` keeps the stored one. */
  saveRemoteAgent(r: RemoteAgent): Promise<RemoteAgent>;
  /** Refused (409) while a team uses it. */
  deleteRemoteAgent(id: string): Promise<void>;
  teams(): Promise<TeamDef[]>;
  saveTeam(t: TeamDef): Promise<TeamDef>;
  /** A team can always be deleted; the sessions that use it can no longer continue. */
  deleteTeam(id: string): Promise<void>;

  sessions(): Promise<Session[]>;
  getSession(id: string): Promise<Session>;
  createSession(input: { agent_id: string; title?: string; isolation?: "none" | "worktree" }): Promise<Session>;
  renameSession(id: string, title: string): Promise<Session>;
  deleteSession(id: string): Promise<void>;
  worktreeStatus(sessionId: string): Promise<WorktreeStatus>;
  worktreeApply(sessionId: string): Promise<WorktreeStatus>;
  worktreeDiscard(sessionId: string): Promise<void>;
  /** Checkpoints of the runs of a session, newest first (only runs that saved or created files). */
  sessionCheckpoints(sessionId: string): Promise<CheckpointInfo[]>;
  /** Restores the originals saved by a run and deletes the files it created. */
  rewindRun(runId: string): Promise<RewindResult>;
  messages(sessionId: string): Promise<ChatMessage[]>;
  /** The ratings of a session (thumbs up/down on assistant messages). */
  sessionFeedback(sessionId: string): Promise<Feedback[]>;
  /** Rates an assistant message; rating again replaces the previous rating (and its note). */
  setFeedback(sessionId: string, messageId: string, rating: 1 | -1, note?: string): Promise<Feedback>;
  deleteFeedback(sessionId: string, messageId: string): Promise<void>;
  /** The lessons the agents proposed, most recent first. */
  lessons(status?: LessonStatus): Promise<Lesson[]>;
  /** Approving writes the lesson (AGENTS.md or a new skill); title and content can be changed first. */
  approveLesson(id: string, edit?: { title?: string; content?: string }): Promise<Lesson>;
  rejectLesson(id: string): Promise<Lesson>;
  /** Asks the agent for a lesson on this conversation now (the answer waits for the model). */
  reflectSession(sessionId: string): Promise<Lesson>;
  /** What the assistant remembers: pinned first, then the most recently changed (a search `q` ranks by relevance). */
  memories(query?: MemoryQuery): Promise<Memory[]>;
  createMemory(body: MemoryCreate): Promise<Memory>;
  updateMemory(id: string, patch: MemoryPatch): Promise<Memory>;
  /** Soft delete: `restoreMemory` brings it back. */
  forgetMemory(id: string): Promise<Memory>;
  restoreMemory(id: string): Promise<Memory>;
  /** Confirms what the assistant proposed. */
  approveMemory(id: string): Promise<Memory>;
  /** Discards a proposal for good. */
  rejectMemory(id: string): Promise<Memory>;
  /** Deletes for good. */
  deleteMemory(id: string): Promise<void>;
  /** Deletes every memory for good; returns how many. */
  deleteAllMemories(): Promise<{ deleted: number }>;
  /** What the proactive assistant noticed, newest first (`status`/`kind` narrow it). */
  proposals(query?: ProposalQuery): Promise<Proposal[]>;
  /** Approving starts a normal run in a new session; `instruction` replaces the proposed one. */
  approveProposal(id: string, instruction?: string): Promise<Proposal>;
  dismissProposal(id: string): Promise<Proposal>;
  proactiveStatus(): Promise<ProactiveStatus>;
  /** Wakes the assistant once, now (the same gates apply: off, quiet hours, budget). */
  proactiveTick(): Promise<TickResult>;
  /** The same check under a hard time budget, as the phone's operating system wakes it (POST /api/proactive/background-tick). */
  proactiveBackgroundTick(req?: BackgroundRequest): Promise<BackgroundSummary>;
  /**
   * The phone's side of the background checks (only the Tauri shell on iOS/Android, and the demo): what the system
   * allows, asking for the notification permission, scheduling the checks from the settings, and «Controlla adesso»
   * as a job that also finishes when the app is left.
   */
  backgroundStatus?(): Promise<BackgroundStatus>;
  backgroundRequestNotifications?(): Promise<BackgroundStatus>;
  backgroundConfigure?(): Promise<BackgroundStatus>;
  backgroundCheckNow?(): Promise<BackgroundStatus>;
  /** `kind: "a2ui.action"` when an A2UI surface reports a user action. */
  /** `thinking` is the reasoning of this run only: it wins over the profile's. */
  /** `attachments`: files sent with the message (the text may be empty when there are some). */
  startRun(sessionId: string, input: string, kind?: "a2ui.action", thinking?: ThinkingMode, attachments?: AttachmentUpload[]): Promise<{ run_id: string }>;
  /** An attachment of a stored message (thumbnails, downloads). 404 when it is gone. */
  getAttachment(sessionId: string, attachmentId: string): Promise<AttachmentData>;
  cancelRun(runId: string): Promise<void>;
  subscribeRun(runId: string, onEvent: (e: AgEvent) => void): () => void;
  activeRun(sessionId: string): Promise<string | null>;
  /** The events of a run so far (a sub-agent's run included), in order: the trace of a card. */
  runEvents(runId: string): Promise<AgEvent[]>;
  approvals(): Promise<ApprovalRequest[]>;
  respondApproval(id: string, decision: "allow" | "deny", remember?: boolean): Promise<void>;

  workflows(): Promise<WorkflowDef[]>;
  saveWorkflow(w: WorkflowDef): Promise<WorkflowDef>;
  deleteWorkflow(id: string): Promise<void>;
  runWorkflow(id: string, inputs?: Record<string, unknown>): Promise<WorkflowRun>;
  workflowRuns(): Promise<WorkflowRun[]>;

  evalSuites(): Promise<EvalSuite[]>;
  /** The server checks the suite (tasks, graders, agents and profiles named) and refuses it with the reason. */
  saveEvalSuite(s: EvalSuite): Promise<EvalSuite>;
  deleteEvalSuite(id: string): Promise<void>;
  /** Starts a run in the background: the report comes back as `running` and is polled with `evalReport`. */
  startEval(suiteId: string, req?: EvalRunRequest): Promise<EvalReport>;
  evalReports(): Promise<EvalReport[]>;
  evalReport(id: string): Promise<EvalReport>;
  compareEvalReports(a: string, b: string): Promise<EvalComparison>;
  /** Tries variants of the agent's prompt on the suite in the background: the record comes back as `running` and is polled with `promptOptimization`. Nothing is applied. */
  startPromptOptimization(suiteId: string, req: OptimizeRequest): Promise<PromptOptimization>;
  promptOptimizations(): Promise<PromptOptimization[]>;
  promptOptimization(id: string): Promise<PromptOptimization>;
  /** Saves the prompt of a variant (index in `variants`) as the agent's system prompt. */
  applyPromptOptimization(id: string, variant: number): Promise<PromptOptimization>;

  /** Device, trainers and what can be trained here. */
  trainDoctor(): Promise<TrainDoctor>;
  installTrainer(id: string): Promise<InstallJob>;
  trainerInstall(id: string): Promise<InstallJob | null>;
  datasets(): Promise<DatasetInfo[]>;
  /** The server refuses with the reason when there are too few examples. */
  buildDataset(spec: DatasetSpec): Promise<DatasetInfo>;
  importSftDataset(name: string, jsonl: string): Promise<DatasetInfo>;
  deleteDataset(id: string): Promise<void>;
  trainJobs(): Promise<TrainJob[]>;
  /** Starts a fine-tuning in the background: the job comes back `queued` and is polled with `trainJobs`. */
  startTrainJob(req: TrainJobRequest): Promise<TrainJob>;
  cancelTrainJob(id: string): Promise<TrainJob>;
  /** The last `lines` lines of the log. */
  trainJobLog(id: string, lines?: number): Promise<string[]>;
  adapters(): Promise<AdapterInfo[]>;
  deleteAdapter(id: string): Promise<void>;

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
