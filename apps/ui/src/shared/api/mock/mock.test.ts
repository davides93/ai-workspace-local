import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DownloadProgress } from "../types";
import { MockTransport } from "./index";

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

  it("re-detects runtimes", async () => {
    vi.useFakeTimers();
    const p = api.refreshRuntimes();
    await vi.advanceTimersByTimeAsync(1000);
    expect((await p).length).toBeGreaterThan(5);
    vi.useRealTimers();
  });
});
