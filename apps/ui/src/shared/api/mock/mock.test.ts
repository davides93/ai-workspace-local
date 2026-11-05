import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SECRET_MASK, type DownloadProgress } from "../types";
import { MockTransport, urlDest } from "./index";

// Basic contract of the demo transport for the model hub (docs/architecture/model-hub.md §3-§4).

describe("MockTransport capabilities", () => {
  it("is a demo carrier", () => {
    const api = new MockTransport("nota");
    expect(api.kind).toBe("mock");
    expect(api.capabilities).toEqual({ demo: true, native: false, pairing: false });
    expect(api.notice).toBe("nota");
  });
});

describe("model hub: providers and search", () => {
  const api = new MockTransport();

  it("lists the five providers with their auth model", async () => {
    const list = await api.modelProviders();
    expect(list.map((p) => p.id)).toEqual(["catalog", "hf", "modelscope", "ollama", "url"]);
    const hf = list.find((p) => p.id === "hf")!;
    expect(hf.auth).toBe("optional");
    expect(hf.capabilities).toContain("multi_file");
    expect(hf.formats).toEqual(expect.arrayContaining(["gguf", "mlx", "onnx", "safetensors"]));
    expect(list.find((p) => p.id === "ollama")!.auth).toBe("none");
  });

  it("stores a provider token without ever returning it", async () => {
    const r = await api.setProviderAuth("modelscope", "secret-token", "https://mirror.example");
    expect(r.auth_configured).toBe(true);
    expect(r.endpoint).toBe("https://mirror.example");
    expect(JSON.stringify(r)).not.toContain("secret-token");
    expect((await api.setProviderAuth("modelscope", null)).auth_configured).toBe(false);
    await expect(api.setProviderAuth("ollama", "x")).rejects.toThrow();
  });

  it("searches Hugging Face and respects the format filter", async () => {
    const all = await api.searchModels({ provider: "hf", q: "", limit: 50 });
    expect(all.items.length).toBeGreaterThanOrEqual(10);
    expect(all.items.every((m) => m.provider === "hf")).toBe(true);
    expect(all.items[0].downloads!).toBeGreaterThanOrEqual(all.items[1].downloads!);
    const mlx = await api.searchModels({ provider: "hf", q: "", format: "mlx" });
    expect(mlx.items.length).toBeGreaterThan(0);
    expect(mlx.items.every((m) => m.formats.includes("mlx"))).toBe(true);
    const qwen = await api.searchModels({ provider: "hf", q: "qwen" });
    expect(qwen.items.length).toBeGreaterThan(0);
    expect(qwen.items.every((m) => /qwen/i.test(`${m.id} ${m.name} ${m.publisher}`) || m.tags.some((t) => /qwen/i.test(t)))).toBe(true);
  });

  it("covers about twenty results across hf, modelscope and ollama", async () => {
    const counts = await Promise.all(["hf", "modelscope", "ollama"].map(async (provider) => (await api.searchModels({ provider, q: "", limit: 50 })).items.length));
    const total = counts.reduce((a, b) => a + b, 0);
    expect(total).toBeGreaterThanOrEqual(20);
    expect(total).toBeLessThanOrEqual(30);
  });

  it("pages with a cursor", async () => {
    const first = await api.searchModels({ provider: "hf", q: "", limit: 5 });
    expect(first.items).toHaveLength(5);
    expect(first.next_cursor).toBeDefined();
    const second = await api.searchModels({ provider: "hf", q: "", limit: 5, cursor: first.next_cursor });
    expect(second.items).toHaveLength(5);
    expect(second.items[0].id).not.toBe(first.items[0].id);
  });

  it("searches the curated catalog and returns nothing for the url provider", async () => {
    expect((await api.searchModels({ provider: "catalog", q: "gemma" })).items.length).toBeGreaterThan(0);
    expect((await api.searchModels({ provider: "url", q: "x" })).items).toEqual([]);
  });

  it("accepts an exact owner/name repository on Hugging Face", async () => {
    const r = await api.searchModels({ provider: "hf", q: "someone/Brand-New-7B-GGUF" });
    expect(r.items).toHaveLength(1);
    expect(r.items[0].id).toBe("someone/Brand-New-7B-GGUF");
  });
});

describe("model hub: remote model variants", () => {
  const api = new MockTransport();

  it("offers Q4_K_M (recommended), Q5_K_M, Q8_0 and F16 for a GGUF repository", async () => {
    const d = await api.remoteModel("hf", "Qwen/Qwen3-4B-GGUF");
    expect(d.model.id).toBe("Qwen/Qwen3-4B-GGUF");
    expect(d.revision).toBe("main");
    expect(d.variants.map((v) => v.id)).toEqual(["Q4_K_M", "Q5_K_M", "Q8_0", "F16"]);
    expect(d.variants.filter((v) => v.recommended)).toHaveLength(1);
    expect(d.variants.find((v) => v.recommended)!.id).toBe("Q4_K_M");
    for (const v of d.variants) {
      expect(v.size_bytes).toBe(v.files.reduce((s, f) => s + f.size_bytes, 0));
      expect(["great", "ok", "tight", "no"]).toContain(v.fit);
      expect(v.runtimes.length).toBeGreaterThan(0);
      expect(v.runtimes.some((r) => r.available)).toBe(true);
      expect(v.files.every((f) => f.role === "weights")).toBe(true);
    }
    // Sizes grow with the precision.
    const sizes = d.variants.map((v) => v.size_bytes);
    expect([...sizes].sort((a, b) => a - b)).toEqual(sizes);
  });

  it("adds the mmproj file to every variant of a vision model", async () => {
    const d = await api.remoteModel("hf", "google/gemma-4-27b-it-GGUF");
    expect(d.model.capabilities).toContain("vision");
    for (const v of d.variants) expect(v.files.some((f) => f.role === "mmproj")).toBe(true);
  });

  it("splits very large variants into shards and marks what does not fit", async () => {
    const d = await api.remoteModel("hf", "bartowski/Llama-3.3-70B-Instruct-GGUF");
    const f16 = d.variants.find((v) => v.id === "F16")!;
    expect(f16.files.length).toBeGreaterThan(1);
    expect(f16.files[0].path).toMatch(/-00001-of-0000\d\.gguf$/);
    expect(f16.fit).toBe("no");
    // Only a variant that fits comfortably (or at least fits) is ever recommended: at most one.
    expect(d.variants.filter((v) => v.recommended).length).toBeLessThanOrEqual(1);
    expect(d.variants.filter((v) => v.recommended).every((v) => v.fit === "great" || v.fit === "ok")).toBe(true);
  });

  it("returns the MLX repository as a single variant that needs macOS", async () => {
    const d = await api.remoteModel("hf", "mlx-community/Qwen3-4B-4bit");
    expect(d.variants).toHaveLength(1);
    const v = d.variants[0];
    expect(v.id).toBe("repo");
    expect(v.format).toBe("mlx");
    expect(v.fit).toBe("no");
    expect(v.runtimes.every((r) => !r.available)).toBe(true);
    expect(v.runtimes.find((r) => r.id === "mlx-lm")!.reason).toMatch(/macOS/);
    expect(v.files.map((f) => f.role)).toEqual(expect.arrayContaining(["config", "tokenizer", "weights"]));
  });

  it("describes Ollama tags with their projector", async () => {
    const d = await api.remoteModel("ollama", "llava:7b");
    expect(d.variants[0].files.map((f) => f.path)).toEqual(["model.gguf", "mmproj.gguf"]);
    expect(d.variants.some((v) => v.recommended)).toBe(true);
  });

  it("rejects an unknown provider", async () => {
    await expect(api.remoteModel("nope", "x/y")).rejects.toThrow();
  });
});

describe("model hub: downloads", () => {
  let api: MockTransport;
  beforeEach(() => {
    vi.useFakeTimers();
    api = new MockTransport();
  });
  afterEach(() => vi.useRealTimers());

  const find = async (id: string): Promise<DownloadProgress> => (await api.downloads()).find((d) => d.id === id)!;
  const sleep = async (ms: number) => void (await vi.advanceTimersByTimeAsync(ms));

  it("walks a multi-file job through queued/running/verifying/done and registers the model", async () => {
    const before = (await api.models()).length;
    const job = await api.pullModel({ provider: "hf", model: "google/gemma-4-27b-it-GGUF", variant: "Q4_K_M" });
    expect(job.provider).toBe("hf");
    expect(job.model_ref).toBe("google/gemma-4-27b-it-GGUF");
    expect(job.variant).toBe("Q4_K_M");
    expect(job.files_total).toBe(2);
    expect(job.total).toBeGreaterThan(10 * 1024 ** 3);
    expect(["queued", "running"]).toContain(job.status);

    await sleep(2000);
    const mid = await find(job.id);
    expect(mid.status).toBe("running");
    expect(mid.received).toBeGreaterThan(0);
    expect(mid.received).toBeLessThan(mid.total);
    expect(mid.speed_bps).toBeGreaterThan(0);

    await sleep(8000);
    const verifying = await find(job.id);
    expect(["verifying", "done"]).toContain(verifying.status);
    expect(verifying.files_done).toBeGreaterThanOrEqual(1);
    await sleep(2000);
    const done = await find(job.id);
    expect(done.status).toBe("done");
    expect(done.received).toBe(done.total);
    expect(done.files_done).toBe(done.files_total);

    const models = await api.models();
    expect(models.length).toBe(before + 1);
    const added = models.find((m) => m.id === "gemma-4-27b-it-gguf-q4_k_m")!;
    expect(added.origin).toBe("aiw");
    expect(added.removable).toBe(true);
    expect(added.runtime_compat.length).toBeGreaterThan(0);
    // The variant is now flagged as installed.
    const [detail] = await Promise.all([api.remoteModel("hf", "google/gemma-4-27b-it-GGUF"), vi.advanceTimersByTimeAsync(500)]);
    expect(detail.variants.find((v) => v.id === "Q4_K_M")!.installed).toBe(true);
  });

  it("cancels a running job and clears finished ones", async () => {
    const job = await api.pullModel({ provider: "ollama", model: "qwen3:4b", variant: "4b" });
    await sleep(1000);
    expect((await find(job.id)).status).toBe("running");
    const cancelled = await api.cancelDownload(job.id);
    expect(cancelled.status).toBe("cancelled");
    await sleep(5000);
    expect((await find(job.id)).received).toBeLessThan(cancelled.total);
    await api.clearDownload(job.id);
    expect(await find(job.id)).toBeUndefined();
  });

  it("refuses to clear an active job", async () => {
    const job = await api.pullModel("gemma-4-27b");
    await expect(api.clearDownload(job.id)).rejects.toThrow();
    await api.cancelDownload(job.id);
  });

  it("runs at most two jobs at once and queues the rest", async () => {
    const a = await api.pullModel({ url: "https://example.org/a.gguf" });
    const b = await api.pullModel({ url: "https://example.org/b.gguf" });
    const c = await api.pullModel({ url: "https://example.org/c.gguf" });
    const list = await api.downloads();
    expect(list.filter((d) => d.status === "running")).toHaveLength(2);
    expect((await find(c.id)).status).toBe("queued");
    await api.cancelDownload(a.id);
    await api.cancelDownload(b.id);
    expect((await find(c.id)).status).toBe("running");
    await api.cancelDownload(c.id);
  });

  it("reports a gated repository without a token as an error right away", async () => {
    const job = await api.pullModel({ provider: "hf", model: "meta-llama/Llama-3.2-3B-Instruct", variant: "repo" });
    const d = await find(job.id);
    expect(d.status).toBe("error");
    expect(d.error).toMatch(/gated/);
  });

  it("surfaces a failing transfer as an error with a message", async () => {
    const job = await api.pullModel({ url: "https://example.org/fail.gguf" });
    await sleep(4000);
    const d = await find(job.id);
    expect(d.status).toBe("error");
    expect(d.error).toBeTruthy();
  });

  it("accepts the legacy {repo,file} form and the catalog id string", async () => {
    const legacy = await api.pullModel({ repo: "Qwen/Qwen3-4B-GGUF", file: "Qwen3-4B-Q4_K_M.gguf" });
    expect(legacy.provider).toBe("hf");
    expect(legacy.file).toBe("Qwen3-4B-Q4_K_M.gguf");
    await api.cancelDownload(legacy.id);
    await expect(api.pullModel("apple-fm-system")).rejects.toThrow(/sistema/);
    await expect(api.pullModel("qwen3-coder-480b")).rejects.toThrow();
  });

  it("deletes only removable models", async () => {
    const models = await api.models();
    const external = models.find((m) => !m.removable)!;
    await expect(api.deleteModel(external.id)).rejects.toThrow();
    const mine = models.find((m) => m.id === "qwen3-embedding-0.6b-q8_0")!;
    expect(mine.removable).toBe(true);
    await api.deleteModel(mine.id);
    expect((await api.models()).some((m) => m.id === mine.id)).toBe(false);
    await expect(api.deleteModel("does-not-exist")).rejects.toThrow();
  });
});

describe("other v0.4 methods", () => {
  const api = new MockTransport();

  it("round-trips policy and hooks", async () => {
    const { rules } = await api.policy();
    expect(rules.length).toBeGreaterThan(0);
    expect(rules.every((r) => ["allow", "ask", "deny"].includes(r.action))).toBe(true);
    const saved = await api.setPolicy([...rules, { tool: "web_fetch", pattern: "*.example.org", action: "deny" }]);
    expect(saved.rules).toHaveLength(rules.length + 1);
    expect((await api.policy()).rules.at(-1)!.tool).toBe("web_fetch");
    await api.setPolicy(rules);

    const { hooks } = await api.hooks();
    expect(hooks[0]).toMatchObject({ event: expect.any(String), command: expect.any(String), timeout_s: expect.any(Number) });
    expect((await api.setHooks([])).hooks).toEqual([]);
    expect((await api.hooks()).hooks).toEqual([]);
    await api.setHooks(hooks);
  });

  it("renames, reads and deletes agents, profiles, sessions and workflows", async () => {
    const renamed = await api.renameSession("ses_pdf", "  Nuovo titolo ");
    expect(renamed.title).toBe("Nuovo titolo");
    expect((await api.getSession("ses_pdf")).title).toBe("Nuovo titolo");
    await expect(api.renameSession("ses_pdf", "  ")).rejects.toThrow();
    await expect(api.getSession("nope")).rejects.toThrow();

    await api.saveProfile({ id: "p-tmp", name: "Temporaneo", runtime_id: "llama-server", model: "gemma-4-e4b-q4_k_m", params: {} });
    await api.saveAgent({ id: "a-tmp", name: "Tmp", description: "", inference_profile: "p-tmp", system_prompt: "", tools: [], mode: "ask", sandbox: "read-only", max_turns: 5 });
    await expect(api.deleteProfile("p-tmp")).rejects.toThrow(/Tmp/);
    await api.deleteAgent("a-tmp");
    await api.deleteProfile("p-tmp");
    expect((await api.agents()).some((a) => a.id === "a-tmp")).toBe(false);
    expect((await api.profiles()).some((p) => p.id === "p-tmp")).toBe(false);

    await api.saveWorkflow({ id: "wf-tmp", name: "Tmp", enabled: false, trigger: { kind: "manual" }, inputs: {}, steps: [] });
    await api.deleteWorkflow("wf-tmp");
    expect((await api.workflows()).some((w) => w.id === "wf-tmp")).toBe(false);
  });

  it("keeps remote agents like the engine: the token is masked and kept, and a team protects the agent", async () => {
    const demo = await api.remoteAgents();
    expect(demo[0]).toMatchObject({ id: "studio", token: SECRET_MASK });
    const base = { id: "tmp", name: "Tmp", url: "http://h:7777", token: "segreto", description: "" };
    await expect(api.saveRemoteAgent({ ...base, id: "a b" })).rejects.toMatchObject({ status: 400 });
    await expect(api.saveRemoteAgent({ ...base, id: "peer-x" })).rejects.toMatchObject({ message: expect.stringContaining("riservati") });
    await expect(api.saveRemoteAgent({ ...base, name: " " })).rejects.toMatchObject({ message: "il nome dell'agente remoto è obbligatorio" });
    await expect(api.saveRemoteAgent({ ...base, url: "ftp://x" })).rejects.toMatchObject({ message: expect.stringContaining("http(s)") });
    const saved = await api.saveRemoteAgent(base);
    expect(saved.token).toBe(SECRET_MASK);
    // Sending the mask back keeps the stored token: nothing leaks, nothing is lost.
    await api.saveRemoteAgent({ ...saved, description: "nuova" });
    expect((await api.remoteAgents()).find((r) => r.id === "tmp")).toMatchObject({ token: SECRET_MASK, description: "nuova" });

    await api.saveTeam({ id: "t-rem", name: "Con remoto", description: "", strategy: "parallel", members: [{ agent: "remote:tmp", role: "" }] });
    await expect(api.deleteRemoteAgent("tmp")).rejects.toMatchObject({ status: 409, message: expect.stringContaining("Con remoto") });
    await api.deleteTeam("t-rem");
    await api.deleteRemoteAgent("tmp");
    await expect(api.deleteRemoteAgent("tmp")).rejects.toMatchObject({ status: 404 });
  });

  it("saves, validates and deletes teams like the engine, and protects the agents they use", async () => {
    const demo = await api.teams();
    expect(demo.map((t) => t.id)).toContain("revisione");
    const base = { id: "t-tmp", name: "Tmp", description: "", strategy: "pipeline" as const, members: [{ agent: "coder", role: "scrive" }] };
    await expect(api.saveTeam({ ...base, strategy: "coro" as never })).rejects.toMatchObject({ status: 400, message: expect.stringContaining("strategia non valida") });
    await expect(api.saveTeam({ ...base, members: [] })).rejects.toMatchObject({ message: "un team ha da 1 a 8 membri" });
    await expect(api.saveTeam({ ...base, members: [base.members[0], base.members[0]] })).rejects.toMatchObject({ message: expect.stringContaining("compare più volte") });
    await expect(api.saveTeam({ ...base, members: [{ agent: "ghost", role: "" }] })).rejects.toMatchObject({ message: "l'agente `ghost` non esiste" });
    await expect(api.saveTeam({ ...base, strategy: "supervisor" })).rejects.toMatchObject({ message: expect.stringContaining("richiede un supervisore") });
    await expect(api.saveTeam({ ...base, strategy: "supervisor", supervisor: "coder" })).rejects.toMatchObject({ message: expect.stringContaining("non può essere anche un membro") });
    await expect(api.saveTeam({ ...base, budget: { max_tokens: 0 } })).rejects.toMatchObject({ message: expect.stringContaining("budget.max_tokens") });
    await expect(api.saveTeam({ ...base, members: [{ agent: "remote:r1", role: "" }] })).resolves.toBeDefined();

    await api.saveTeam({ ...base, members: [{ agent: "browser", role: "" }] });
    await expect(api.deleteAgent("browser")).rejects.toMatchObject({ status: 409, message: expect.stringMatching(/usato dai team: .*Tmp/) });
    await api.deleteTeam("t-tmp");
    await expect(api.deleteTeam("t-tmp")).rejects.toMatchObject({ status: 404 });
    expect((await api.teams()).some((t) => t.id === "t-tmp")).toBe(false);
  });

  it("re-detects runtimes", async () => {
    vi.useFakeTimers();
    const p = api.refreshRuntimes();
    await vi.advanceTimersByTimeAsync(1000);
    expect((await p).length).toBeGreaterThan(5);
    vi.useRealTimers();
  });
});

// What the mockup shows of the backend fixes of v0.4 (crates/aiw-core/src/api.rs, aiw-engine).
describe("backend fixes in the demo", () => {
  it("lists the tools of the agent loop as internal, always on, and refuses to switch them (400)", async () => {
    const api = new MockTransport();
    const internal = (await api.tools()).filter((t) => t.source === "internal");
    expect(internal.map((t) => t.name).sort()).toEqual(["agent", "code", "handoff", "render_ui", "skill", "team_state", "todo"]);
    expect(internal.every((t) => t.enabled && t.available && t.risk === "read")).toBe(true);
    await expect(api.setToolEnabled("todo", false)).rejects.toMatchObject({ status: 400, message: expect.stringContaining("strumento interno") });
    await expect(api.setToolEnabled("nope", false)).rejects.toMatchObject({ status: 404 });
    await api.setToolEnabled("shell", false);
    await api.setToolEnabled("shell", true);
  });

  it("masks the secrets of MCP servers and keeps the stored value when the mask comes back", async () => {
    const api = new MockTransport();
    const gh = (await api.mcpServers()).find((s) => s.name === "github")!;
    expect(gh.headers).toEqual({ Authorization: "••••••" });
    const pg = (await api.mcpServers()).find((s) => s.name === "postgres")!;
    expect(pg.env).toEqual({ PGPASSWORD: "••••••" });
    const fs = (await api.mcpServers()).find((s) => s.name === "filesystem")!;
    expect(fs.env).toEqual({ LOG_LEVEL: "info" });
    // Saved back unchanged (and with an added header): the stored token survives, the new header is kept.
    const saved = await api.saveMcpServer({ ...gh, headers: { ...gh.headers, Accept: "application/json" } });
    expect(saved.headers).toEqual({ Authorization: "••••••", Accept: "application/json" });
    // Changed: the new value replaces the stored one (and is hidden again on the way out).
    const changed = await api.saveMcpServer({ ...gh, headers: { Authorization: "Bearer nuovo" } });
    expect(changed.headers).toEqual({ Authorization: "••••••" });
    // A mask with nothing behind it is dropped.
    const fresh = await api.saveMcpServer({ name: "x", transport: "http", url: "http://127.0.0.1:1/mcp", headers: { "X-Token": "••••••" } });
    expect(fresh.headers).toEqual({});
  });

  it("validates an MCP server like the engine", async () => {
    const api = new MockTransport();
    await expect(api.saveMcpServer({ name: " ", transport: "stdio", command: "npx" })).rejects.toMatchObject({ status: 400, message: "serve un nome" });
    await expect(api.saveMcpServer({ name: "a", transport: "stdio" })).rejects.toMatchObject({ message: "un server MCP stdio richiede il comando da avviare" });
    await expect(api.saveMcpServer({ name: "a", transport: "http" })).rejects.toMatchObject({ message: "un server MCP http richiede l'indirizzo (url)" });
    await expect(api.saveMcpServer({ name: "a", transport: "http", url: "ftp://x" })).rejects.toMatchObject({ message: expect.stringContaining("indirizzo MCP non valido: ftp://x") });
    await expect(api.saveMcpServer({ name: "a", transport: "ws" as never })).rejects.toMatchObject({ message: expect.stringContaining("non valido: usa stdio o http") });
  });

  it("validates an agent like the engine", async () => {
    const api = new MockTransport();
    const base = (await api.agents())[0];
    await expect(api.saveAgent({ ...base, max_turns: 0 })).rejects.toMatchObject({ status: 400, message: "max_turns deve essere almeno 1" });
    await expect(api.saveAgent({ ...base, inference_profile: "ghost" })).rejects.toMatchObject({ message: "il profilo di inferenza `ghost` non esiste" });
    await expect(api.saveAgent({ ...base, tools: ["read_file", "writ_file", "ghost"] })).rejects.toMatchObject({ message: expect.stringContaining("strumenti sconosciuti: writ_file, ghost") });
    await expect(api.saveAgent({ ...base, tools: ["*", "mcp__x__y", "todo", "render_ui", "code"] })).resolves.toBeDefined();
  });

  it("answers 404 for a session that is gone", async () => {
    const api = new MockTransport();
    const s = await api.createSession({ agent_id: "operator" });
    await expect(api.messages(s.id)).resolves.toEqual([]);
    await api.deleteSession(s.id);
    await expect(api.messages(s.id)).rejects.toMatchObject({ status: 404 });
    await expect(api.activeRun(s.id)).rejects.toMatchObject({ status: 404 });
    await expect(api.startRun(s.id, "ciao")).rejects.toMatchObject({ status: 404 });
    await expect(api.deleteSession(s.id)).rejects.toMatchObject({ status: 404 });
    await expect(api.removeMcpServer("nope")).rejects.toMatchObject({ status: 404 });
  });

  it("has a session whose history ends with the persisted run error", async () => {
    const api = new MockTransport();
    const msgs = await api.messages("ses_bug");
    const last = msgs[msgs.length - 1];
    expect(last).toMatchObject({ role: "system", name: "aiw.run_error" });
    expect(last.content).toMatch(/^Errore del runtime: /);
    expect((await api.sessions()).find((s) => s.id === "ses_bug")?.last_status).toBe("failed");
  });
});

describe("the scripted browser run", () => {
  it("sends one aiw.image (a PNG, base64 without prefix) for the screenshot tool, before its result", async () => {
    vi.useFakeTimers();
    try {
      const api = new MockTransport();
      const { run_id } = await api.startRun("ses_cv", "Apri la pagina dei preventivi");
      const events: import("../types").AgEvent[] = [];
      api.subscribeRun(run_id, (e) => events.push(e));
      await vi.advanceTimersByTimeAsync(20_000);
      expect(events.at(-1)?.type).toBe("RUN_FINISHED");
      const images = events.filter((e) => e.type === "CUSTOM" && e.name === "aiw.image");
      expect(images).toHaveLength(1);
      const v = (images[0] as { value: { toolCallId: string; mime: string; data: string } }).value;
      expect(v.mime).toBe("image/png");
      expect(v.data.startsWith("data:")).toBe(false);
      expect([...atob(v.data).slice(0, 8)].map((c) => c.charCodeAt(0).toString(16).padStart(2, "0")).join("")).toBe("89504e470d0a1a0a");
      const start = events.findIndex((e) => e.type === "TOOL_CALL_START" && e.toolCallId === v.toolCallId);
      const result = events.findIndex((e) => e.type === "TOOL_CALL_RESULT" && e.toolCallId === v.toolCallId);
      const at = events.indexOf(images[0]);
      expect(start).toBeLessThan(at);
      expect(at).toBeLessThan(result);
      expect(events.find((e) => e.type === "TOOL_CALL_START" && e.toolCallId === v.toolCallId)).toMatchObject({ toolCallName: "browser_screenshot" });
    } finally {
      vi.useRealTimers();
    }
  });

  it("leaves the other agents' scripted run without screenshots", async () => {
    vi.useFakeTimers();
    try {
      const api = new MockTransport();
      const { run_id } = await api.startRun("ses_pdf", "Ordina i PDF");
      const events: import("../types").AgEvent[] = [];
      api.subscribeRun(run_id, (e) => events.push(e));
      await vi.advanceTimersByTimeAsync(2_000);
      expect(events.some((e) => e.type === "CUSTOM" && e.name === "aiw.image")).toBe(false);
      expect(events.some((e) => e.type === "TOOL_CALL_START")).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("direct URL downloads", () => {
  it("land in a folder of their own per URL: url/<host>[_<port>]/<name>-<hash>/<file>", () => {
    const a = urlDest("https://modelli.example.org/m/qwen3-4b-Q4_K_M.gguf", "qwen3-4b-Q4_K_M.gguf");
    const b = urlDest("https://modelli.example.org/altro/qwen3-4b-Q4_K_M.gguf", "qwen3-4b-Q4_K_M.gguf");
    expect(a).toMatch(/^~\/\.aiw\/models\/url\/modelli\.example\.org\/qwen3-4b-Q4_K_M-[0-9a-f]{8}\/qwen3-4b-Q4_K_M\.gguf$/);
    expect(a).not.toBe(b);
    expect(urlDest("http://192.168.1.5:8080/x/m.gguf", "m.gguf")).toMatch(/\/url\/192\.168\.1\.5_8080\/m-[0-9a-f]{8}\/m\.gguf$/);
  });
});

describe("voice mode (demo)", () => {
  it("reports an available backend with the voice settings", async () => {
    const api = new MockTransport();
    const v = await api.voice();
    expect(v.available).toBe(true);
    expect(v.runtime).toBe("whisper-server");
    expect(v.models).toContain("ggml-large-v3-turbo-q5_0");
    expect(v.language).toBe("it");
    const view = await api.config();
    expect(Object.keys(view.schema.properties).filter((k) => k.startsWith("voice."))).toEqual(["voice.language", "voice.stt_runtime", "voice.stt_model", "voice.tts_voice"]);
  });

  it("transcribes with a plausible sentence per request and the duration of the audio", async () => {
    vi.useFakeTimers();
    try {
      const api = new MockTransport();
      const wav = new Blob([new Uint8Array(44 + 32_000)], { type: "audio/wav" });
      const first = api.transcribe(wav);
      await vi.advanceTimersByTimeAsync(700);
      const a = await first;
      expect(a.text).not.toBe("");
      expect(a.duration_ms).toBe(1000);
      const second = api.transcribe(wav);
      await vi.advanceTimersByTimeAsync(700);
      expect((await second).text).not.toBe(a.text);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the speech models and runtime out of the chat profiles' choices (they are listed as models)", async () => {
    const api = new MockTransport();
    const whisper = (await api.models()).find((m) => m.id === "ggml-large-v3-turbo-q5_0")!;
    expect(whisper.capabilities).toEqual(["stt"]);
    expect(whisper.runtime_compat.map((c) => c.id)).toEqual(["whisper-server"]);
  });
});

describe("runtime installation (demo)", () => {
  it("shows the plan, installs by itself and leaves the runtime ready", async () => {
    vi.useFakeTimers();
    try {
      const api = new MockTransport();
      const plan = await api.runtimeInstallPlan("vllm");
      expect(plan.automatic).toBe(true);
      expect(plan.steps.length).toBe(2);
      expect(await api.runtimeInstall("vllm")).toBeNull();
      const job = await api.installRuntime("vllm");
      expect(job.state).toBe("running");
      await vi.advanceTimersByTimeAsync(2000);
      const done = await api.runtimeInstall("vllm");
      expect(done?.state).toBe("done");
      expect(done?.log.at(-1)).toContain("completata");
      expect((await api.runtimes()).find((r) => r.id === "vllm")?.status).toBe("available");
    } finally {
      vi.useRealTimers();
    }
  });

  it("refuses runtimes that are installed by hand, and unknown ones", async () => {
    const api = new MockTransport();
    expect((await api.runtimeInstallPlan("lmstudio")).automatic).toBe(false);
    await expect(api.installRuntime("lmstudio")).rejects.toThrow(/manuale/);
    await expect(api.installRuntime("nope")).rejects.toThrow(/non trovato/);
  });
});

describe("updates (demo)", () => {
  it("checks, downloads with progress, then hands the installer over", async () => {
    vi.useFakeTimers();
    try {
      const api = new MockTransport();
      const first = await api.updateStatus();
      expect(first.state).toBe("idle");
      expect(first.available ?? null).toBeNull();
      await expect(api.downloadUpdate()).rejects.toThrow(/controlla prima/);
      await expect(api.installUpdate()).rejects.toThrow(/verificato/);
      const check = api.checkUpdate();
      await vi.advanceTimersByTimeAsync(700);
      const found = await check;
      expect(found.available?.version).toBe("0.4.0-rc.3");
      expect(found.checked_at).toBeTruthy();
      expect((await api.downloadUpdate()).state).toBe("downloading");
      await vi.advanceTimersByTimeAsync(2500);
      const ready = await api.updateStatus();
      expect(ready.state).toBe("ready");
      expect(ready.received).toBe(ready.total);
      expect(ready.file).toContain("0.4.0-rc.3");
      expect((await api.installUpdate()).message).toMatch(/Applicazioni/);
    } finally {
      vi.useRealTimers();
    }
  });

  it("lists the update settings in the demo configuration", async () => {
    const view = await new MockTransport().config();
    expect(view.schema.properties["update.channel"].enum).toEqual(["auto", "stable", "beta", "alpha"]);
    expect(view.effective["update.auto_check"]).toBe(true);
    expect(view.effective["update.auto_download"]).toBe(false);
  });
});
