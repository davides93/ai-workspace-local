import type { Transport, TransportCapabilities } from "../transport";
import { ApiError } from "../http";
import { isSecretName } from "../../lib/secrets";
import { SECRET_MASK } from "../types";
import type {
  AgEvent, AgentProfile, ApprovalRequest, DownloadProgress, HookConfig, InferenceProfile, McpServer,
  MeshState, ModelProviderInfo, ModelSearchParams, ModelVariant, PolicyRule, PullRequest, RemoteFile, RemoteModel, RemoteModelDetail, RunState,
  SearchPage, Session, Transcript, VoiceStatus, WorkflowDef, WorkflowRun, WorktreeStatus,
} from "../types";
import {
  agents, appliedWorktrees, audit, catalog, compatForFormat, configLayers, configSchema, DEMO_SCREENSHOT_PNG, effective, hookConfigs, INTERNAL_TOOL_NAMES, mcp, models, nid, now, policyRules, profiles,
  runtimes, sessionMessages, sessions, sleep, system, tools, workflowRuns, workflows, MOCK_DIFF, GB, fitFor,
} from "./fixtures";
import { buildDetail, corpus, fakeSha, providers, searchCorpus } from "./hub";

/** A simulated download: files transferred one after the other, then verified. */
interface MockJob {
  progress: DownloadProgress;
  files: RemoteFile[];
  /** Action to run when the job completes (registers the model). */
  onDone: () => void;
  /** Simulated failure after this many ticks (demo of the error state). */
  failAfter?: { tick: number; message: string };
  ticks: number;
  timer?: ReturnType<typeof setInterval>;
}

const MAX_ACTIVE_JOBS = 2;
const TICKS_TO_COMPLETE = 22;
const TICK_MS = 400;

/**
 * Where a direct-URL download lands: every URL has its own folder, `url/<host>[_<port>]/<name without extension>-<8 hex
 * digits of the URL's hash>/<file>`, so two URLs of the same host are two models (the real hash is a sha256).
 */
export function urlDest(url: string, file: string): string {
  let host = "host";
  try {
    const u = new URL(url);
    host = u.port ? `${u.hostname}_${u.port}` : u.hostname;
  } catch { /* not a URL: the form already refused it */ }
  return `~/.aiw/models/url/${host}/${file.replace(/\.[^.]+$/, "")}-${fakeSha(url).slice(0, 8)}/${file}`;
}

/** The headers and variables the engine hides when it lists a server, as `GET /api/mcp/servers` does. */
function maskSecrets(rec: Record<string, unknown> | undefined): Record<string, unknown> | undefined {
  if (!rec) return rec;
  return Object.fromEntries(Object.entries(rec).map(([k, v]) => [k, isSecretName(k) && v !== null && v !== "" ? SECRET_MASK : v]));
}

/** What a client sent with the masks it received put back to the stored value (a mask with nothing behind it is dropped). */
function restoreSecrets(sent: Record<string, unknown> | undefined, stored: Record<string, unknown> | undefined): Record<string, unknown> | undefined {
  if (!sent) return sent;
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(sent)) {
    if (v !== SECRET_MASK) out[k] = v;
    else if (stored && k in stored) out[k] = stored[k];
  }
  return out;
}

/** `validate_mcp` of the engine, with its messages. */
function validateMcp(s: { name: string; transport: string; command?: string; url?: string }): void {
  if (!s.name.trim()) throw new ApiError(400, "serve un nome");
  if (s.transport === "stdio") {
    if (!s.command?.trim()) throw new ApiError(400, "un server MCP stdio richiede il comando da avviare");
  } else if (s.transport === "http") {
    const url = s.url?.trim();
    if (!url) throw new ApiError(400, "un server MCP http richiede l'indirizzo (url)");
    let ok = false;
    try {
      const u = new URL(url);
      ok = (u.protocol === "http:" || u.protocol === "https:") && u.hostname !== "";
    } catch { /* not a URL */ }
    if (!ok) throw new ApiError(400, `indirizzo MCP non valido: ${url} (serve un URL http o https)`);
  } else {
    throw new ApiError(400, `trasporto MCP \`${s.transport}\` non valido: usa stdio o http`);
  }
}

/** What the demo "hears", one sentence per request. */
const VOICE_DEMO_SENTENCES = ["Quali file ho modificato oggi nel workspace?", "Riassumi in due righe il preventivo più recente", "Grazie, per ora è tutto"];

export class MockTransport implements Transport {
  readonly kind = "mock" as const;
  readonly capabilities: TransportCapabilities = { demo: true, native: false, pairing: false };
  private runListeners = new Map<string, Set<(e: AgEvent) => void>>();
  private runEvents = new Map<string, AgEvent[]>();
  private pendingApprovals = new Map<string, { req: ApprovalRequest; resolve: (allow: boolean) => void }>();
  private jobs: MockJob[] = [];
  private voiceTurn = 0;
  private installedVariants = new Set<string>();
  private variantByModel = new Map<string, string>();
  private providerAuth = new Map<string, { configured: boolean; endpoint?: string }>();

  constructor(public readonly notice?: string) {}

  async system() { return structuredClone(system); }
  async kill() {
    for (const [, p] of this.pendingApprovals) p.resolve(false);
    audit.unshift({ id: nid("a"), ts: now(), actor: "user", action: "kill", detail: "Kill switch premuto dalla UI" });
  }

  async runtimes() { return structuredClone(runtimes); }
  async refreshRuntimes() {
    await sleep(700);
    audit.unshift({ id: nid("a"), ts: now(), actor: "user", action: "runtime.refresh", detail: "Rilevamento di tutti i runtime" });
    return structuredClone(runtimes);
  }
  async runtimeAction(id: string, action: "start" | "stop" | "refresh", opts?: { model?: string }) {
    const r = runtimes.find((x) => x.id === id);
    if (!r) throw new Error(`runtime ${id} non trovato`);
    if (action === "start") {
      if (r.status === "unsupported" || r.status === "installable") throw new Error(`${r.name}: ${r.status === "unsupported" ? "non supportato su questo device" : "non installato"}`);
      r.status = "starting";
      await sleep(900);
      r.status = "running";
      r.status_detail = r.accelerators[0]?.toUpperCase() + " · pronto";
      if (opts?.model) r.loaded_models = [opts.model];
    } else if (action === "stop") {
      r.status = "stopped";
      r.status_detail = undefined;
      r.loaded_models = [];
    }
    audit.unshift({ id: nid("a"), ts: now(), actor: "user", action: `runtime.${action}`, detail: r.name });
    return structuredClone(r);
  }
  async updateRuntimeSettings(id: string, settings: Record<string, unknown>) {
    const r = runtimes.find((x) => x.id === id)!;
    r.settings = { ...r.settings, ...settings };
    return structuredClone(r);
  }

  async models() { return structuredClone(models); }
  async catalog() { return structuredClone(catalog); }

  // ---------------------------------------------------------------- model hub

  async modelProviders(): Promise<ModelProviderInfo[]> {
    return providers.map((p) => ({ ...p, auth_configured: this.providerAuth.get(p.id)?.configured ?? p.auth_configured, endpoint: this.providerAuth.get(p.id)?.endpoint ?? p.endpoint }));
  }
  /** Same semantics as the server: token absent = unchanged, null = removed; endpoint absent = unchanged, "" = default. */
  async setProviderAuth(id: string, token: string | null | undefined, endpoint?: string): Promise<ModelProviderInfo> {
    const p = providers.find((x) => x.id === id);
    if (!p) throw new Error(`Provider ${id} non trovato`);
    if (typeof token === "string" && !p.capabilities.includes("auth")) throw new Error(`${p.name} non usa un token`);
    if (p.endpoint === "" && endpoint) throw new Error(`${p.name} non ha impostazioni di accesso`);
    await sleep(250);
    const cur = this.providerAuth.get(id);
    this.providerAuth.set(id, {
      configured: token === undefined ? (cur?.configured ?? false) : token !== null,
      endpoint: endpoint === undefined ? cur?.endpoint : endpoint || undefined,
    });
    return (await this.modelProviders()).find((x) => x.id === id)!;
  }

  /** Catalog entries as remote models, so the curated list is searchable like any provider. */
  private catalogAsRemote(): RemoteModel[] {
    return catalog.map((c) => ({
      provider: "catalog", id: c.id, name: c.name, publisher: c.publisher, description: c.description, formats: [c.format], tags: c.tags,
      capabilities: c.capabilities, license: c.license, gated: false, params_b: c.params_b, url: "",
    }));
  }

  async searchModels(p: ModelSearchParams): Promise<SearchPage> {
    await sleep(350);
    if (p.provider === "url") return { items: [] };
    const items = p.provider === "catalog" ? this.catalogAsRemote() : corpus.filter((m) => m.provider === p.provider);
    const page = searchCorpus(items, p);
    // ModelScope's search is best effort: an exact `owner/name` is always accepted.
    if (page.items.length === 0 && (p.provider === "hf" || p.provider === "modelscope") && /^[\w.-]+\/[\w.-]+$/.test(p.q.trim())) {
      const d = buildDetail(items, p.provider, p.q.trim(), this.installedVariants);
      return { items: [d.model] };
    }
    return page;
  }

  async remoteModel(provider: string, model: string, revision?: string): Promise<RemoteModelDetail> {
    await sleep(300);
    if (provider === "catalog") {
      const c = catalog.find((x) => x.id === model);
      if (!c) throw new Error(`Modello ${model} non trovato nel catalogo`);
      const variant: ModelVariant = {
        id: c.quant || "default", label: `${c.quant || "default"} · ${c.format}`, format: c.format, quant: c.quant, size_bytes: c.size_bytes, fit: c.fit, fit_detail: c.fit_detail, recommended: c.fit === "great",
        files: c.source.kind === "hf" ? [{ path: c.source.file, size_bytes: c.size_bytes, role: "weights" }] : [], runtimes: c.runtime_compat, installed: c.installed,
      };
      return { model: this.catalogAsRemote().find((m) => m.id === c.id)!, revision: "catalogo", variants: [variant] };
    }
    // A gated repository lists no files without a token (HTTP 401), as the real Hub does.
    const hit = corpus.find((x) => x.provider === provider && x.id === model);
    if (hit?.gated && !this.providerAuth.get(provider)?.configured) {
      throw Object.assign(new Error("repository gated: serve un token Hugging Face con accesso a questo modello"), { status: 401 });
    }
    return buildDetail(corpus, provider, model, this.installedVariants, revision);
  }

  async downloads() { return this.jobs.map((j) => structuredClone(j.progress)); }

  async pullModel(req: string | PullRequest): Promise<DownloadProgress> {
    const job = this.newJob(typeof req === "string" ? { id: req } : req);
    this.jobs.unshift(job);
    this.pump();
    return structuredClone(job.progress);
  }

  private newJob(req: PullRequest): MockJob {
    const base = { received: 0, status: "queued" as const, speed_bps: 0, files_done: 0 };
    if ("id" in req) {
      const c = catalog.find((x) => x.id === req.id);
      if (!c) throw new Error(`Modello ${req.id} non trovato nel catalogo`);
      if (c.source.kind === "system") throw new Error("Questo è un modello di sistema: viene fornito dal runtime e non si scarica.");
      if (c.fit === "no") throw new Error(`${c.name} non ci sta su questo device: ${c.fit_detail}`);
      const repo = c.source.repo;
      const files: RemoteFile[] = [{ path: c.source.file, size_bytes: c.size_bytes, role: "weights" }];
      return this.makeJob({ ...base, id: nid("dl"), model_id: c.id, provider: "catalog", model_ref: repo, variant: c.quant, file: files[0].path, total: c.size_bytes, files_total: 1, dest: `~/.aiw/models/hf/${repo}`, files },
        () => {
          c.installed = true;
          models.push({
            id: `${c.id}-${c.quant.toLowerCase()}`, name: c.name, format: c.format, quant: c.quant, size_bytes: c.size_bytes, params_b: c.params_b, context_length: c.context_length, capabilities: c.capabilities,
            runtimes: c.runtime_compat.filter((r) => r.available).map((r) => r.id), source: `hf:${repo}`, origin: "aiw", removable: true, runtime_compat: c.runtime_compat, fit: c.fit, fit_detail: c.fit_detail,
          });
        });
    }
    if ("provider" in req) {
      const detail = buildDetail(corpus, req.provider, req.model, this.installedVariants, req.revision);
      const v = detail.variants.find((x) => x.id === req.variant);
      if (!v) throw new Error(`Variante ${req.variant} non trovata in ${req.model}`);
      if (detail.model.gated && !this.providerAuth.get(req.provider)?.configured) {
        const job = this.makeJob({ ...base, id: nid("dl"), model_id: `${req.provider}/${req.model}`, provider: req.provider, model_ref: req.model, variant: v.id, file: v.files[0]?.path ?? "", total: v.size_bytes, files_total: v.files.length, dest: "", files: v.files }, () => undefined);
        job.failAfter = { tick: 0, message: "repository gated: serve un token Hugging Face (Modelli → Scopri → provider)" };
        return job;
      }
      const key = `${req.provider}/${req.model}/${v.id}`;
      return this.makeJob({ ...base, id: nid("dl"), model_id: key, provider: req.provider, model_ref: req.model, variant: v.id, file: v.files[0]?.path ?? "", total: v.size_bytes, files_total: v.files.length, dest: `~/.aiw/models/${req.provider}/${req.model}`, files: v.files },
        () => {
          this.installedVariants.add(key);
          const compatRt = v.runtimes;
          const localId = `${detail.model.name}-${v.id}`.toLowerCase();
          this.variantByModel.set(localId, key);
          models.push({
            id: localId, name: `${detail.model.name} (${v.label})`, format: v.format, quant: v.quant, size_bytes: v.size_bytes, params_b: detail.model.params_b,
            context_length: 32768, capabilities: detail.model.capabilities, runtimes: compatRt.filter((r) => r.available).map((r) => r.id), source: `${req.provider}:${req.model}`, origin: "aiw", removable: true,
            runtime_compat: compatRt, fit: v.fit, fit_detail: v.fit_detail,
          });
        });
    }
    if ("repo" in req) {
      const size = 4.4 * GB;
      return this.makeJob({ ...base, id: nid("dl"), model_id: `hf/${req.repo}/${req.file}`, provider: "hf", model_ref: req.repo, variant: req.file, file: req.file, total: size, files_total: 1, dest: `~/.aiw/models/hf/${req.repo}`, files: [{ path: req.file, size_bytes: size, role: "weights" }] },
        () => {
          const f = fitFor(size);
          models.push({ id: req.file.replace(/\.gguf$/i, "").toLowerCase(), name: req.file, format: "gguf", size_bytes: size, capabilities: ["chat"], runtimes: ["llama-server"], source: `hf:${req.repo}`, origin: "aiw", removable: true, runtime_compat: compatForFormat("gguf"), fit: f.fit, fit_detail: f.detail });
        });
    }
    const name = req.file_name ?? (req.url.split("/").pop() || "modello.gguf");
    const size = 2.1 * GB;
    const job = this.makeJob({ ...base, id: nid("dl"), model_id: `url/${name}`, provider: "url", model_ref: req.url, variant: "", file: name, total: size, files_total: 1, dest: urlDest(req.url, name), files: [{ path: name, size_bytes: size, sha256: req.sha256, role: "weights" }] },
      () => {
        const f = fitFor(size);
        models.push({ id: name.replace(/\.gguf$/i, "").toLowerCase(), name, format: "gguf", size_bytes: size, capabilities: ["chat"], runtimes: ["llama-server"], source: `url:${req.url}`, origin: "aiw", removable: true, runtime_compat: compatForFormat("gguf"), fit: f.fit, fit_detail: f.detail });
      });
    // Demo of a failing transfer: any URL containing "fail".
    if (/fail/i.test(req.url)) job.failAfter = { tick: 6, message: "connessione interrotta dal server (dopo 3 tentativi)" };
    return job;
  }

  private makeJob(progress: Omit<DownloadProgress, "error"> & { files: RemoteFile[] }, onDone: () => void): MockJob {
    const { files, ...p } = progress;
    return { progress: p, files, onDone, ticks: 0 };
  }

  /** Starts queued jobs while fewer than two are active. */
  private pump() {
    for (const job of this.jobs.slice().reverse()) {
      if (job.progress.status !== "queued") continue;
      if (this.jobs.filter((j) => j.progress.status === "running" || j.progress.status === "verifying").length >= MAX_ACTIVE_JOBS) break;
      this.startJob(job);
    }
  }

  private startJob(job: MockJob) {
    const d = job.progress;
    d.status = "running";
    if (job.failAfter?.tick === 0) {
      d.status = "error";
      d.error = job.failAfter.message;
      this.pump();
      return;
    }
    const step = Math.max(1, d.total / TICKS_TO_COMPLETE);
    job.timer = setInterval(() => {
      if (d.status !== "running") return;
      job.ticks++;
      if (job.failAfter && job.ticks >= job.failAfter.tick) {
        d.status = "error";
        d.error = job.failAfter.message;
        d.speed_bps = 0;
        clearInterval(job.timer);
        this.pump();
        return;
      }
      const before = d.received;
      d.received = Math.min(d.total, d.received + step);
      d.speed_bps = Math.round(((d.received - before) / TICK_MS) * 1000);
      let acc = 0;
      d.files_done = 0;
      for (const f of job.files) {
        acc += f.size_bytes;
        if (d.received >= acc && acc < d.total) d.files_done++;
        else {
          d.file = f.path;
          break;
        }
      }
      if (d.received >= d.total) {
        clearInterval(job.timer);
        d.files_done = d.files_total;
        d.status = "verifying";
        d.speed_bps = 0;
        setTimeout(() => {
          if (d.status !== "verifying") return;
          d.status = "done";
          job.onDone();
          this.pump();
        }, 700);
      }
    }, TICK_MS);
  }

  async cancelDownload(id: string): Promise<DownloadProgress> {
    const job = this.jobs.find((j) => j.progress.id === id);
    if (!job) throw new Error("Download non trovato");
    if (job.progress.status === "queued" || job.progress.status === "running" || job.progress.status === "verifying") {
      if (job.timer) clearInterval(job.timer);
      job.progress.status = "cancelled";
      job.progress.speed_bps = 0;
      this.pump();
    }
    return structuredClone(job.progress);
  }

  async clearDownload(id: string): Promise<void> {
    const i = this.jobs.findIndex((j) => j.progress.id === id);
    if (i < 0) return;
    const s = this.jobs[i].progress.status;
    if (s === "queued" || s === "running" || s === "verifying") throw new Error("Il download è ancora in corso: annullalo prima di rimuoverlo");
    this.jobs.splice(i, 1);
  }

  async deleteModel(id: string): Promise<void> {
    const i = models.findIndex((m) => m.id === id);
    if (i < 0) throw new Error("Modello non trovato");
    const m = models[i];
    if (!m.removable) throw new Error(`${m.name} appartiene a un'altra app (${m.origin}): non si può eliminare da qui`);
    models.splice(i, 1);
    const c = catalog.find((x) => `${x.id}-${x.quant.toLowerCase()}` === id);
    if (c) c.installed = false;
    const key = this.variantByModel.get(id);
    if (key) this.installedVariants.delete(key);
    audit.unshift({ id: nid("a"), ts: now(), actor: "user", action: "model.delete", detail: m.name });
  }

  async profiles() { return structuredClone(profiles); }
  async saveProfile(p: InferenceProfile) {
    const i = profiles.findIndex((x) => x.id === p.id);
    if (i >= 0) profiles[i] = p; else profiles.push(p);
    return p;
  }
  async deleteProfile(id: string) {
    const user = agents.find((a) => a.inference_profile === id);
    if (user) throw new Error(`Il profilo è usato dall'agente "${user.name}": cambia prima il suo profilo`);
    const i = profiles.findIndex((p) => p.id === id);
    if (i >= 0) profiles.splice(i, 1);
  }
  async agents() { return structuredClone(agents); }
  async saveAgent(a: AgentProfile) {
    // `save_agent` of the engine, with its messages.
    if (!["plan", "ask", "auto-edit", "auto", "yolo"].includes(a.mode)) throw new ApiError(400, `modalità non valida: ${a.mode}`);
    if (!["read-only", "workspace-write", "off"].includes(a.sandbox)) throw new ApiError(400, `sandbox non valida: ${a.sandbox}`);
    if (a.max_turns < 1) throw new ApiError(400, "max_turns deve essere almeno 1");
    if (!profiles.some((p) => p.id === a.inference_profile)) throw new ApiError(400, `il profilo di inferenza \`${a.inference_profile}\` non esiste`);
    const unknown = a.tools.filter((t) => t !== "*" && !t.startsWith("mcp__") && !tools.some((x) => x.name === t));
    if (unknown.length > 0) throw new ApiError(400, `strumenti sconosciuti: ${unknown.join(", ")} (usa "*" oppure i nomi elencati in Strumenti)`);
    const i = agents.findIndex((x) => x.id === a.id);
    if (i >= 0) agents[i] = a; else agents.push(a);
    return a;
  }

  async deleteAgent(id: string) {
    if (sessions.some((s) => s.agent_id === id)) throw new Error("L'agente ha delle sessioni: eliminale prima");
    const i = agents.findIndex((a) => a.id === id);
    if (i >= 0) agents.splice(i, 1);
  }

  async sessions() { return structuredClone(sessions); }
  async getSession(id: string) {
    const s = sessions.find((x) => x.id === id);
    if (!s) throw new Error("Sessione non trovata");
    return structuredClone(s);
  }
  async renameSession(id: string, title: string) {
    const s = sessions.find((x) => x.id === id);
    if (!s) throw new Error("Sessione non trovata");
    const t = title.trim();
    if (!t) throw new Error("Il titolo non può essere vuoto");
    s.title = t;
    s.updated_at = now();
    return structuredClone(s);
  }
  async createSession(input: { agent_id: string; title?: string; isolation?: "none" | "worktree" }) {
    const id = nid("ses");
    const short = Math.random().toString(16).slice(2, 10);
    const s: Session = {
      id, title: input.title ?? "Nuova sessione", agent_id: input.agent_id, created_at: now(), updated_at: now(),
      worktree: input.isolation === "worktree"
        ? { path: `~/.aiw/worktrees/progetto-${short}`, root: `~/.aiw/worktrees/progetto-${short}`, branch: `aiw/${short}`, base: "9c39159", repo: "~/progetto" }
        : undefined,
    };
    sessions.unshift(s);
    return s;
  }
  async worktreeStatus(sessionId: string): Promise<WorktreeStatus> {
    const s = sessions.find((x) => x.id === sessionId);
    if (!s?.worktree) throw new Error("Questa sessione non usa un worktree.");
    const changed = sessionId === "ses_bug" && !appliedWorktrees.has(sessionId);
    return {
      ...s.worktree,
      files: changed ? [{ status: "M", path: "crates/aiw-models/src/gguf.rs" }, { status: "A", path: "crates/aiw-models/tests/header.rs" }] : [],
      diff_stat: changed ? " crates/aiw-models/src/gguf.rs   | 5 +++--\n crates/aiw-models/tests/header.rs | 6 ++++++\n 2 files changed, 9 insertions(+), 2 deletions(-)\n" : "",
      diff: changed ? MOCK_DIFF : "",
      truncated: false,
    };
  }
  async worktreeApply(sessionId: string) {
    const st = await this.worktreeStatus(sessionId);
    if (st.files.length === 0) throw new Error("Nessuna modifica da applicare.");
    appliedWorktrees.add(sessionId);
    return this.worktreeStatus(sessionId);
  }
  async worktreeDiscard(sessionId: string) {
    const s = sessions.find((x) => x.id === sessionId);
    if (s) s.worktree = undefined;
  }
  async deleteSession(id: string) {
    const i = sessions.findIndex((s) => s.id === id);
    if (i < 0) throw new ApiError(404, `sessione ${id} non trovata`);
    sessions.splice(i, 1);
  }
  // 404 for a session that is gone, like the backend (the UI leaves a conversation that was deleted elsewhere).
  async messages(sessionId: string) {
    if (!sessions.some((s) => s.id === sessionId)) throw new ApiError(404, `sessione ${sessionId} non trovata`);
    return structuredClone(sessionMessages[sessionId] ?? []);
  }

  async activeRun(sessionId?: string) {
    if (sessionId !== undefined && !sessions.some((s) => s.id === sessionId)) throw new ApiError(404, `sessione ${sessionId} non trovata`);
    return null;
  }
  async runtimeLogs(id: string) {
    return id === "llama-server"
      ? ["main: server is listening on http://127.0.0.1:39281", "srv  load_model: loaded qwen3.5-35b-a3b-q4_k_m (CUDA0: 21.4 GiB)", "slot launch_slot_: id 0 | task 12 | processing task", "slot print_timing: eval time = 1912 ms / 214 tokens (112.0 tokens per second)"]
      : [];
  }
  async approvals() { return [...this.pendingApprovals.values()].map((p) => p.req); }
  async respondApproval(id: string, decision: "allow" | "deny") {
    const p = this.pendingApprovals.get(id);
    if (!p) return;
    this.pendingApprovals.delete(id);
    audit.unshift({ id: nid("a"), ts: now(), run_id: p.req.run_id, actor: "user", action: "approval", tool: p.req.tool, decision: decision === "allow" ? "approved" : "rejected", detail: JSON.stringify(p.req.args) });
    p.resolve(decision === "allow");
  }

  subscribeRun(runId: string, onEvent: (e: AgEvent) => void) {
    (this.runEvents.get(runId) ?? []).forEach(onEvent);
    const set = this.runListeners.get(runId) ?? new Set();
    set.add(onEvent);
    this.runListeners.set(runId, set);
    return () => set.delete(onEvent);
  }
  async cancelRun(runId: string) {
    this.emit(runId, { type: "RUN_ERROR", message: "Run annullato dall'utente", code: "cancelled" });
  }

  private emit(runId: string, e: AgEvent) {
    const ev = { ...e, timestamp: Date.now() } as AgEvent;
    const list = this.runEvents.get(runId) ?? [];
    list.push(ev);
    this.runEvents.set(runId, list);
    this.runListeners.get(runId)?.forEach((fn) => fn(ev));
  }

  async startRun(sessionId: string, input: string, kind?: "a2ui.action") {
    if (!sessions.some((s) => s.id === sessionId)) throw new ApiError(404, `sessione ${sessionId} non trovata`);
    const runId = nid("run");
    const msgs = sessionMessages[sessionId] ?? (sessionMessages[sessionId] = []);
    msgs.push({ id: nid("m"), role: "user", content: input, name: kind, created_at: now() });
    const s = sessions.find((x) => x.id === sessionId);
    if (s && s.title === "Nuova sessione" && !kind) s.title = input.slice(0, 48);
    if (kind === "a2ui.action") void this.actionReply(runId, sessionId, input);
    else void this.scriptedRun(runId, sessionId, input);
    return { run_id: runId };
  }

  private async actionReply(runId: string, threadId: string, input: string) {
    await sleep(150);
    this.emit(runId, { type: "RUN_STARTED", threadId, runId });
    let ctx: Record<string, unknown> = {};
    let name = "";
    try {
      const v = JSON.parse(input.replace(/^\[Azione A2UI\]\s*/, "")) as { action: { name: string; context: Record<string, unknown> } };
      ctx = v.action.context ?? {};
      name = v.action.name;
    } catch { /* demo */ }
    const m = nid("m");
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m, role: "assistant" });
    const sala = Array.isArray(ctx.sala) ? String(ctx.sala[0] ?? "") : String(ctx.sala ?? "");
    await this.streamText(runId, m, name === "prenota"
      ? `Fatto: sala **${sala.charAt(0).toUpperCase() + sala.slice(1)}** prenotata per ${String(ctx.quando ?? "").replace("T", " alle ")}, ${String(ctx.persone ?? "")} persone${ctx.proiettore ? ", con proiettore" : ""}. Ho aggiunto l'evento al calendario locale.`
      : "Va bene, non prenoto nulla.", "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m });
    this.emit(runId, { type: "RUN_FINISHED", threadId, runId });
  }

  private async streamText(runId: string, messageId: string, text: string, kind: "text" | "reasoning") {
    const words = text.split(/(\s+)/);
    for (let i = 0; i < words.length; i += 3) {
      const delta = words.slice(i, i + 3).join("");
      this.emit(runId, kind === "text" ? { type: "TEXT_MESSAGE_CONTENT", messageId, delta } : { type: "REASONING_MESSAGE_CONTENT", messageId, delta });
      await sleep(kind === "text" ? 28 : 18);
    }
  }

  /** `image`: a screenshot the tool produced, sent as the `aiw.image` custom event (base64 data, no `data:` prefix). */
  private async toolCall(runId: string, name: string, args: unknown, result: string, approval?: { reason: string; risk: ApprovalRequest["risk"] }, image?: { mime: string; data: string }) {
    const toolCallId = nid("call");
    this.emit(runId, { type: "TOOL_CALL_START", toolCallId, toolCallName: name });
    this.emit(runId, { type: "TOOL_CALL_ARGS", toolCallId, delta: JSON.stringify(args) });
    this.emit(runId, { type: "TOOL_CALL_END", toolCallId });
    if (image) this.emit(runId, { type: "CUSTOM", name: "aiw.image", value: { toolCallId, mime: image.mime, data: image.data } });
    if (approval) {
      const id = nid("apr");
      const req: ApprovalRequest = { id, run_id: runId, tool: name, args, reason: approval.reason, risk: approval.risk };
      const allowed = await new Promise<boolean>((resolve) => {
        this.pendingApprovals.set(id, { req, resolve });
        this.emit(runId, { type: "CUSTOM", name: "aiw.approval_request", value: req });
      });
      this.emit(runId, { type: "CUSTOM", name: "aiw.approval_resolved", value: { id, allowed } });
      if (!allowed) {
        this.emit(runId, { type: "TOOL_CALL_RESULT", messageId: nid("m"), toolCallId, content: "Negato dall'utente." });
        return false;
      }
    } else {
      await sleep(350);
    }
    this.emit(runId, { type: "TOOL_CALL_RESULT", messageId: nid("m"), toolCallId, content: result });
    audit.unshift({ id: nid("a"), ts: now(), run_id: runId, actor: "agent", action: "tool", tool: name, decision: approval ? "approved" : "allow", detail: JSON.stringify(args).slice(0, 120) });
    return true;
  }

  /** A session of the browser agent: open a page, take its screenshot (an `aiw.image` event), read it, answer. */
  private async browserRun(runId: string, sessionId: string) {
    await sleep(150);
    const threadId = sessionId;
    this.emit(runId, { type: "RUN_STARTED", threadId, runId });
    this.emit(runId, { type: "STATE_SNAPSHOT", snapshot: { runtime: "llama-server", model: "gemma-4-e4b-q4_k_m", usage: { prompt_tokens: 2210, completion_tokens: 160, tokens_per_s: 131, context_used: 2370, context_max: 16384 } } });

    const m1 = nid("m");
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m1, role: "assistant" });
    await this.streamText(runId, m1, "Apro la pagina dei preventivi e guardo com'è fatta.", "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m1 });

    await this.toolCall(runId, "browser_navigate", { url: "https://fornitori.example.com/preventivi" }, "Pagina aperta: «Preventivi fornitori» (200 OK, 3 tabelle).");
    await this.toolCall(runId, "browser_screenshot", {}, "Screenshot della pagina acquisito (256×160, PNG).", undefined, { mime: "image/png", data: DEMO_SCREENSHOT_PNG });
    await this.toolCall(runId, "browser_snapshot", {}, "table «Preventivi»\n  row: Rossi Srl · 12.400 € · 45 gg\n  row: Nordtech · 14.950 € · 20 gg\n  row: Alfa Impianti · 13.700 € · 30 gg");

    const final = "La pagina elenca tre preventivi: **Rossi Srl** è il più economico (12.400 €, 45 gg), **Nordtech** il più rapido (20 gg, 14.950 €). Lo screenshot qui sopra mostra la tabella com'è nel browser.";
    const m2 = nid("m");
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m2, role: "assistant" });
    await this.streamText(runId, m2, final, "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m2 });
    this.emit(runId, { type: "RUN_FINISHED", threadId, runId });
    this.finish(sessionId, final);
  }

  private async scriptedRun(runId: string, sessionId: string, input: string) {
    if (sessions.find((x) => x.id === sessionId)?.agent_id === "browser") return this.browserRun(runId, sessionId);
    await sleep(150);
    const threadId = sessionId;
    this.emit(runId, { type: "RUN_STARTED", threadId, runId });
    type Todo = NonNullable<RunState["todos"]>[number];
    const todos: Todo[] = [
      { content: "Elencare i PDF in Downloads", status: "in_progress" },
      { content: "Creare le cartelle per anno e spostare i file", status: "pending" },
      { content: "Scrivere l'indice INDEX.md", status: "pending" },
    ];
    const state = () => ({ todos: structuredClone(todos), runtime: "llama-server", model: "qwen3.5-35b-a3b-q4_k_m", usage: { prompt_tokens: 1840 + Math.round(Math.random() * 400), completion_tokens: 220, tokens_per_s: 108 + Math.round(Math.random() * 10), context_used: 6100, context_max: 32768 } });
    this.emit(runId, { type: "STATE_SNAPSHOT", snapshot: state() });

    const r1 = nid("r");
    this.emit(runId, { type: "REASONING_START", messageId: r1 });
    await this.streamText(runId, r1, `L'utente chiede: "${input.slice(0, 80)}". Prima elenco i PDF, poi li raggruppo per anno dalla data nel nome o di modifica, chiedo conferma per lo spostamento e infine scrivo l'indice.`, "reasoning");
    this.emit(runId, { type: "REASONING_END", messageId: r1 });

    const m1 = nid("m");
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m1, role: "assistant" });
    await this.streamText(runId, m1, "Inizio guardando cosa c'è nella cartella Downloads.", "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m1 });

    await this.toolCall(runId, "list_dir", { path: "~/Downloads", pattern: "*.pdf" },
      "fattura-2024-11-acme.pdf  212 KB  2024-11-03\nfattura-2025-02-acme.pdf  198 KB  2025-02-07\nestratto-conto-2025-06.pdf  1.1 MB  2025-07-01\ncontratto-affitto-2024.pdf  640 KB  2024-01-15\nmanuale-router.pdf  4.2 MB  2025-09-22\nbiglietto-treno-2026-03.pdf  88 KB  2026-03-14\nprogetto-casa-2026.pdf  12.8 MB  2026-05-30");
    todos[0].status = "completed";
    todos[1].status = "in_progress";
    this.emit(runId, { type: "STATE_SNAPSHOT", snapshot: state() });

    const m2 = nid("m");
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m2, role: "assistant" });
    await this.streamText(runId, m2, "Ho trovato 7 PDF tra il 2024 e il 2026. Creo le cartelle `2024`, `2025` e `2026` e ci sposto i file. Per questo comando serve la tua conferma.", "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m2 });

    const ok = await this.toolCall(runId, "shell", { command: "mkdir 2024 2025 2026 && move *2024*.pdf 2024 && move *2025*.pdf 2025 && move *2026*.pdf 2026", cwd: "~/Downloads" },
      "Cartelle create: 3 · File spostati: 6 · Ignorato: manuale-router.pdf (anno non presente nel nome)",
      { reason: "Il comando sposta file nella cartella Downloads.", risk: "medium" });
    if (!ok) {
      const m = nid("m");
      this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m, role: "assistant" });
      await this.streamText(runId, m, "Ok, non sposto nulla. Posso solo preparare l'indice senza modificare la cartella, se vuoi.", "text");
      this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m });
      this.emit(runId, { type: "RUN_FINISHED", threadId, runId });
      this.finish(sessionId, "Ok, non sposto nulla.");
      return;
    }
    todos[1].status = "completed";
    todos[2].status = "in_progress";
    this.emit(runId, { type: "STATE_SNAPSHOT", snapshot: state() });

    const sub = nid("sub");
    this.emit(runId, { type: "SUBAGENT_STARTED", subagentId: sub, name: "Lettore di metadati (Veloce)" });
    await sleep(700);
    this.emit(runId, { type: "SUBAGENT_FINISHED", subagentId: sub, result: "manuale-router.pdf: data documento 2025-09-22 → 2025" });

    await this.toolCall(runId, "write_file", { path: "~/Downloads/INDEX.md", content: "# Indice PDF\n\n## 2024\n- fattura-2024-11-acme.pdf\n- contratto-affitto-2024.pdf\n…" },
      "Scritto INDEX.md (24 righe)");
    todos[2].status = "completed";
    this.emit(runId, { type: "STATE_SNAPSHOT", snapshot: state() });

    const m3 = nid("m");
    const final = "Fatto. I PDF sono ordinati per anno e l'indice è in `Downloads/INDEX.md`.\n\n| Anno | File |\n|---|---|\n| 2024 | 2 |\n| 2025 | 3 (incluso il manuale, datato dai metadati) |\n| 2026 | 2 |\n\nPosso trasformarlo in un workflow che lo rifà ogni lunedì?";
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m3, role: "assistant" });
    await this.streamText(runId, m3, final, "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m3 });
    this.emit(runId, { type: "RUN_FINISHED", threadId, runId });
    this.finish(sessionId, final);
  }

  private finish(sessionId: string, final: string) {
    (sessionMessages[sessionId] ??= []).push({ id: nid("m"), role: "assistant", content: final, created_at: now() });
    const s = sessions.find((x) => x.id === sessionId);
    if (s) {
      s.updated_at = now();
      s.last_status = "succeeded";
    }
  }

  async workflows() { return structuredClone(workflows); }
  async saveWorkflow(w: WorkflowDef) {
    const i = workflows.findIndex((x) => x.id === w.id);
    if (i >= 0) workflows[i] = w; else workflows.push(w);
    return w;
  }
  async deleteWorkflow(id: string) {
    const i = workflows.findIndex((w) => w.id === id);
    if (i >= 0) workflows.splice(i, 1);
  }
  async workflowRuns() { return structuredClone(workflowRuns); }
  async runWorkflow(id: string) {
    const w = workflows.find((x) => x.id === id)!;
    const run: WorkflowRun = { id: nid("wfr"), workflow_id: id, status: "running", started_at: now(), steps: w.steps.map((s) => ({ id: s.id, status: "pending" })) };
    workflowRuns.unshift(run);
    void (async () => {
      for (const st of run.steps) {
        st.status = "running";
        await sleep(900);
        st.status = "done";
        st.output = "ok";
      }
      run.status = "succeeded";
      run.finished_at = now();
    })();
    return structuredClone(run);
  }

  async tools() { return structuredClone(tools); }
  async setToolEnabled(name: string, enabled: boolean) {
    if ((INTERNAL_TOOL_NAMES as readonly string[]).includes(name)) {
      throw new ApiError(400, `«${name}» è uno strumento interno dell'agente: non si può attivare o disattivare`);
    }
    const t = tools.find((x) => x.name === name);
    if (!t) throw new ApiError(404, `tool ${name} non trovato`);
    t.enabled = enabled;
  }
  async mcpServers() {
    return structuredClone(mcp).map((m) => ({ ...m, env: maskSecrets(m.env), headers: maskSecrets(m.headers) }));
  }
  async saveMcpServer(s: Partial<McpServer> & { name: string; transport: "stdio" | "http" }) {
    validateMcp(s);
    // Like the engine: the id decides between create and replace, the name has no spaces, and a secret that comes
    // back as the mask keeps the stored value.
    const i = s.id ? mcp.findIndex((m) => m.id === s.id) : -1;
    const prev = i >= 0 ? mcp[i] : undefined;
    const srv = {
      id: prev?.id ?? nid("mcp"), status: prev?.status ?? "disconnected", tools: prev?.tools ?? [], error: prev?.error, enabled: true, ...s, name: s.name.trim().replace(/ /g, "_"),
      env: restoreSecrets(s.env, prev?.env), headers: restoreSecrets(s.headers, prev?.headers),
    } as McpServer;
    if (i >= 0) mcp[i] = srv;
    else mcp.push(srv);
    return { ...structuredClone(srv), env: maskSecrets(srv.env), headers: maskSecrets(srv.headers) };
  }
  async removeMcpServer(id: string) {
    const i = mcp.findIndex((m) => m.id === id);
    if (i < 0) throw new ApiError(404, `server MCP ${id} non trovato`);
    mcp.splice(i, 1);
  }

  async policy() { return { rules: structuredClone(policyRules) }; }
  async setPolicy(rules: PolicyRule[]) {
    policyRules.splice(0, policyRules.length, ...structuredClone(rules));
    audit.unshift({ id: nid("a"), ts: now(), actor: "user", action: "policy.set", detail: `${rules.length} regole` });
    return { rules: structuredClone(policyRules) };
  }
  async hooks() { return { hooks: structuredClone(hookConfigs) }; }
  async setHooks(hooks: HookConfig[]) {
    hookConfigs.splice(0, hookConfigs.length, ...structuredClone(hooks));
    audit.unshift({ id: nid("a"), ts: now(), actor: "user", action: "hooks.set", detail: `${hooks.length} hook` });
    return { hooks: structuredClone(hookConfigs) };
  }

  async mesh(): Promise<MeshState> {
    return {
      discovery: true,
      address: "http://192.168.1.10:7777",
      discovered: [
        { id: "dev_nas", name: "Mini PC del soggiorno", os: "linux", tier: "m", version: "0.4.0-beta.1", address: "http://192.168.1.31:7777", last_seen: now() },
        { id: "dev_mbp", name: "MacBook Pro M4 Max", os: "macos", tier: "xl", version: "0.4.0-beta.1", address: "http://192.168.1.42:7777", last_seen: now() },
      ],
      peers: structuredClone(system.peers),
    };
  }
  async pairPeer(address: string, code: string) {
    await sleep(500);
    if (!/^\d{6}$/.test(code)) throw new Error("Il codice è di 6 cifre");
    const peer = { id: nid("dev"), name: address.includes(".31") ? "Mini PC del soggiorno" : "Nuovo dispositivo", os: "linux" as const, address, status: "online" as const, shared_runtimes: ["llama-server"] };
    system.peers.push(peer);
    return peer;
  }
  async removePeer(id: string) {
    const i = system.peers.findIndex((p) => p.id === id);
    if (i >= 0) system.peers.splice(i, 1);
  }
  async pairingQr(): Promise<{ url: string; svg: string }> {
    throw new Error("Il QR richiede il backend: avvia aiw serve --lan");
  }

  async voice(): Promise<VoiceStatus> {
    const eff = effective();
    const lang = String(eff["voice.language"] ?? "auto");
    return {
      available: true, runtime: "whisper-server", model: "ggml-large-v3-turbo-q5_0", backend: "whisper.cpp server · ggml-large-v3-turbo-q5_0",
      detail: null, language: lang === "auto" ? "it" : lang, tts_voice: String(eff["voice.tts_voice"] ?? ""), models: ["ggml-large-v3-turbo-q5_0"],
    };
  }
  /** Demo: a plausible sentence per request (the audio itself stays in the browser). */
  async transcribe(wav: Blob): Promise<Transcript> {
    await sleep(650);
    const text = VOICE_DEMO_SENTENCES[this.voiceTurn++ % VOICE_DEMO_SENTENCES.length];
    // 16 kHz mono 16-bit: 32 bytes per millisecond after the 44-byte header.
    return { text, language: "it", backend: "whisper.cpp server · ggml-large-v3-turbo-q5_0", duration_ms: Math.round(Math.max(0, wav.size - 44) / 32), elapsed_ms: 640 };
  }

  async config() { return { layers: structuredClone(configLayers), effective: effective(), schema: configSchema }; }
  async setConfig(layer: "global" | "device" | "workspace", key: string, value: unknown) {
    const l = configLayers.find((x) => x.name === layer)!;
    // `null` removes the key from the layer, as the real config does.
    if (value === null) delete l.values[key];
    else l.values[key] = value;
    return this.config();
  }
  async audit() { return structuredClone(audit); }
}
