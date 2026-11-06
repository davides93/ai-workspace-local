import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgEvent } from "./types";
import { HttpTransport, pullBody, queryString } from "./http";

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((m: { data: string; lastEventId: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  closed = false;
  constructor(public url: string) {
    FakeEventSource.instances.push(this);
  }
  close() {
    this.closed = true;
  }
  emit(seq: number | null, ev: AgEvent) {
    this.onmessage?.({ data: JSON.stringify(ev), lastEventId: seq === null ? "" : String(seq) });
  }
}

describe("queryString / pullBody", () => {
  it("skips empty values and encodes the rest", () => {
    expect(queryString({ provider: "hf", q: "a b", format: undefined, limit: 20, cursor: "" })).toBe("?provider=hf&q=a+b&limit=20");
    expect(queryString({})).toBe("");
  });
  it("sends a string as the catalog id", () => {
    expect(pullBody("qwen3.5-35b-a3b")).toEqual({ id: "qwen3.5-35b-a3b" });
    expect(pullBody({ url: "https://x/y.gguf" })).toEqual({ url: "https://x/y.gguf" });
  });
});

describe("HttpTransport", () => {
  it("is not a demo and supports pairing", () => {
    const t = new HttpTransport("", null);
    expect(t.capabilities).toEqual({ demo: false, native: false, pairing: true });
  });

  it("calls the paths of the contract", async () => {
    const calls: { method: string; url: string; body?: string }[] = [];
    vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
      calls.push({ method: init.method ?? "GET", url, body: init.body as string | undefined });
      return new Response("{}", { status: 200 });
    });
    const t = new HttpTransport("http://h", "tok");
    await t.refreshRuntimes();
    await t.modelProviders();
    await t.setProviderAuth("hf", "t0k", "https://mirror");
    await t.searchModels({ provider: "hf", q: "qwen", format: "gguf", limit: 10 });
    await t.remoteModel("hf", "Qwen/Qwen3-4B-GGUF", "main");
    await t.pullModel("some-id");
    await t.cancelDownload("dl_1");
    await t.clearDownload("dl_1");
    await t.deleteModel("ollama:qwen3:4b");
    await t.deleteAgent("a");
    await t.agentRouting("a/1");
    await t.resetAgentRouting("a/1");
    await t.deleteProfile("p");
    await t.teams();
    await t.saveTeam({ id: "t 1", name: "T", description: "", strategy: "pipeline", members: [{ agent: "a", role: "" }] });
    await t.deleteTeam("t 1");
    await t.renameSession("s", "Titolo");
    await t.getSession("s");
    await t.deleteWorkflow("w");
    await t.policy();
    await t.setPolicy([{ tool: "shell", action: "ask" }]);
    await t.hooks();
    await t.setHooks([]);
    await t.sessionFeedback("s");
    await t.setFeedback("s", "m/1", -1, "troppo lunga");
    await t.setFeedback("s", "m2", 1);
    await t.deleteFeedback("s", "m2");
    await t.evalSuites();
    await t.saveEvalSuite({ id: "s/1", name: "S", tasks: [] });
    await t.deleteEvalSuite("s/1");
    await t.startEval("s/1", { profile: "veloce", repeat: 3 });
    await t.evalReports();
    await t.evalReport("evr 1");
    await t.compareEvalReports("evr_a", "evr b");
    await t.remoteAgents();
    await t.saveRemoteAgent({ id: "r 1", name: "R", url: "http://h:1", token: "", description: "" });
    await t.deleteRemoteAgent("r 1");
    await t.trainDoctor();
    await t.installTrainer("trl");
    await t.trainerInstall("trl");
    await t.datasets();
    await t.buildDataset({ method: "sft", sources: ["feedback"] });
    await t.deleteDataset("ds 1");
    await t.trainJobs();
    await t.startTrainJob({ method: "sft", base_model: "m", dataset_id: "ds_1" });
    await t.cancelTrainJob("trn/1");
    await t.trainJobLog("trn_1", 50);
    await t.adapters();
    await t.deleteAdapter("ad 1");
    vi.unstubAllGlobals();
    expect(calls.map((c) => `${c.method} ${c.url.replace("http://h/api", "")}`)).toEqual([
      "POST /runtimes/refresh",
      "GET /models/providers",
      "PUT /models/providers/hf/auth",
      "GET /models/search?provider=hf&q=qwen&format=gguf&limit=10",
      "GET /models/remote?provider=hf&model=Qwen%2FQwen3-4B-GGUF&revision=main",
      "POST /models/pull",
      "POST /models/downloads/dl_1/cancel",
      "DELETE /models/downloads/dl_1",
      "DELETE /models/ollama%3Aqwen3%3A4b",
      "DELETE /agents/a",
      "GET /agents/a%2F1/routing",
      "DELETE /agents/a%2F1/routing",
      "DELETE /profiles/p",
      "GET /teams",
      "PUT /teams/t%201",
      "DELETE /teams/t%201",
      "PATCH /sessions/s",
      "GET /sessions/s",
      "DELETE /workflows/w",
      "GET /policy",
      "PUT /policy",
      "GET /hooks",
      "PUT /hooks",
      "GET /sessions/s/feedback",
      "PUT /sessions/s/messages/m%2F1/feedback",
      "PUT /sessions/s/messages/m2/feedback",
      "DELETE /sessions/s/messages/m2/feedback",
      "GET /evals",
      "PUT /evals/s%2F1",
      "DELETE /evals/s%2F1",
      "POST /evals/s%2F1/runs",
      "GET /eval-runs",
      "GET /eval-runs/evr%201",
      "GET /eval-runs/compare?a=evr_a&b=evr%20b",
      "GET /remote-agents",
      "PUT /remote-agents/r%201",
      "DELETE /remote-agents/r%201",
      "GET /train/doctor",
      "POST /trainers/trl/install",
      "GET /trainers/trl/install",
      "GET /datasets",
      "POST /datasets",
      "DELETE /datasets/ds%201",
      "GET /train/jobs",
      "POST /train/jobs",
      "POST /train/jobs/trn%2F1/cancel",
      "GET /train/jobs/trn_1/log?lines=50",
      "GET /adapters",
      "DELETE /adapters/ad%201",
    ]);
    expect(JSON.parse(calls[2].body!)).toEqual({ token: "t0k", endpoint: "https://mirror" });
    expect(JSON.parse(calls[5].body!)).toEqual({ id: "some-id" });
    expect(JSON.parse(calls[14].body!)).toMatchObject({ id: "t 1", strategy: "pipeline" });
    expect(JSON.parse(calls[16].body!)).toEqual({ title: "Titolo" });
    expect(JSON.parse(calls[20].body!)).toEqual({ rules: [{ tool: "shell", action: "ask" }] });
    expect(JSON.parse(calls[24].body!)).toEqual({ rating: -1, note: "troppo lunga" });
    expect(JSON.parse(calls[25].body!)).toEqual({ rating: 1 });
    expect(JSON.parse(calls[28].body!)).toMatchObject({ id: "s/1", name: "S" });
    expect(JSON.parse(calls[30].body!)).toEqual({ profile: "veloce", repeat: 3 });
    expect(JSON.parse(calls[41].body!)).toEqual({ method: "sft", sources: ["feedback"] });
    expect(JSON.parse(calls[44].body!)).toEqual({ method: "sft", base_model: "m", dataset_id: "ds_1" });
  });
});

describe("HttpTransport.subscribeRun", () => {
  beforeEach(() => {
    FakeEventSource.instances = [];
    vi.stubGlobal("EventSource", FakeEventSource);
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  const text = (id: string): AgEvent => ({ type: "TEXT_MESSAGE_START", messageId: id, role: "assistant" });

  it("drops events whose seq was already delivered", () => {
    const got: AgEvent[] = [];
    new HttpTransport("", null).subscribeRun("run_1", (e) => got.push(e));
    const es = FakeEventSource.instances[0];
    es.emit(0, { type: "RUN_STARTED", threadId: "t", runId: "run_1" });
    es.emit(1, text("m1"));
    es.emit(1, text("m1"));
    es.emit(0, { type: "RUN_STARTED", threadId: "t", runId: "run_1" });
    es.emit(2, text("m2"));
    expect(got.map((e) => e.type)).toEqual(["RUN_STARTED", "TEXT_MESSAGE_START", "TEXT_MESSAGE_START"]);
  });

  it("reconnects with ?after=<last seq> and skips the replayed history", () => {
    const got: AgEvent[] = [];
    new HttpTransport("", null).subscribeRun("run_1", (e) => got.push(e));
    const first = FakeEventSource.instances[0];
    expect(first.url).toBe("/api/runs/run_1/events");
    first.emit(0, { type: "RUN_STARTED", threadId: "t", runId: "run_1" });
    first.emit(1, text("m1"));
    first.onerror?.();
    expect(first.closed).toBe(true);
    vi.advanceTimersByTime(2000);
    const second = FakeEventSource.instances[1];
    expect(second.url).toBe("/api/runs/run_1/events?after=1");
    // The server may replay everything: only the new event gets through.
    second.emit(0, { type: "RUN_STARTED", threadId: "t", runId: "run_1" });
    second.emit(1, text("m1"));
    second.emit(2, text("m2"));
    expect(got).toHaveLength(3);
  });

  it("passes the token in the query and stops after RUN_FINISHED", () => {
    const got: AgEvent[] = [];
    new HttpTransport("", "tok en").subscribeRun("r", (e) => got.push(e));
    const es = FakeEventSource.instances[0];
    expect(es.url).toBe("/api/runs/r/events?token=tok+en");
    es.emit(0, { type: "RUN_FINISHED", threadId: "t", runId: "r" });
    expect(es.closed).toBe(true);
    es.onerror?.();
    vi.advanceTimersByTime(10_000);
    expect(FakeEventSource.instances).toHaveLength(1);
  });

  it("stops reconnecting when unsubscribed", () => {
    const stop = new HttpTransport("", null).subscribeRun("r", () => undefined);
    FakeEventSource.instances[0].onerror?.();
    stop();
    vi.advanceTimersByTime(10_000);
    expect(FakeEventSource.instances).toHaveLength(1);
  });
});
