import { describe, expect, it } from "vitest";
import { catalog, models, profiles, agents, runtimes } from "../../../shared/api/mock/fixtures";
import type { AgentProfile, CatalogModel, LocalModel, ModelChoice, Recommendations, RuntimeInfo, TaskAdvice } from "../../../shared/api/types";
import { bestInstalledModel, checklist, currentStep, defaultAgent, defaultProfile, firstRunModel, followsAdviser, downloadFraction, firstRunPending, isStartableRuntime, isUsableProfile, isUsableRuntime, profileForModel, recommendedModel, runtimeShortcut, simpleFirstRun, simpleFirstRunPending } from "./overview";

describe("first-run checklist", () => {
  it("lists the four steps in order", () => {
    expect(checklist({ runtimes, models, profiles, sessions: 1 }).map((s) => s.id)).toEqual(["runtime", "model", "profile", "chat"]);
  });
  it("marks the steps that are done", () => {
    const steps = checklist({ runtimes, models: [], profiles, sessions: 0 });
    expect(steps.map((s) => [s.id, s.done])).toEqual([["runtime", true], ["model", false], ["profile", true], ["chat", false]]);
  });
  it("is pending until every step is done, even with a model and a runtime", () => {
    expect(firstRunPending(checklist({ runtimes, models, profiles, sessions: 0 }))).toBe(true);
    expect(firstRunPending(checklist({ runtimes, models, profiles, sessions: 2 }))).toBe(false);
  });
  it("points at the first step still to do", () => {
    expect(currentStep(checklist({ runtimes, models: [], profiles, sessions: 0 }))).toBe("model");
    expect(currentStep(checklist({ runtimes, models, profiles, sessions: 1 }))).toBeUndefined();
  });
  it("needs a running or available runtime", () => {
    const stopped = runtimes.map((r) => ({ ...r, status: r.status === "running" || r.status === "available" ? ("stopped" as const) : r.status }));
    expect(checklist({ runtimes: stopped, models, profiles, sessions: 1 })[0].done).toBe(false);
    expect(runtimes.some(isUsableRuntime)).toBe(true);
  });
});

describe("recommendations", () => {
  it("proposes a downloadable model that fits comfortably", () => {
    const rec = recommendedModel(catalog)!;
    expect(rec.downloadable).toBe(true);
    expect(rec.installed).toBe(false);
    expect(rec.fit).toBe("great");
  });
  it("proposes nothing when everything is installed or unfit", () => {
    expect(recommendedModel(catalog.map((c) => ({ ...c, installed: true })))).toBeUndefined();
  });
  it("picks the profile of the default agent", () => {
    expect(defaultProfile(agents, profiles, "browser")?.id).toBe("fast-local");
    expect(defaultProfile(agents, profiles, undefined)?.id).toBe("balanced");
    expect(defaultProfile([], [], undefined)).toBeUndefined();
  });
});

describe("runtime shortcut", () => {
  const rt = (status: (typeof runtimes)[number]["status"]) => [{ ...runtimes[0], id: "x", status }];
  const profile = { ...profiles[0], runtime_id: "x", model: "m1" };
  it("starts a stopped, available or failed runtime with the profile's model", () => {
    for (const s of ["stopped", "available", "error"] as const) {
      expect(isStartableRuntime(rt(s)[0])).toBe(true);
      expect(runtimeShortcut(rt(s), profile)).toMatchObject({ kind: "start", model: "m1" });
    }
  });
  it("reports a runtime that is already running or starting", () => {
    expect(runtimeShortcut(rt("running"), profile).kind).toBe("running");
    expect(runtimeShortcut(rt("starting"), profile).kind).toBe("running");
  });
  it("sends to the runtimes page when there is nothing to start", () => {
    expect(runtimeShortcut(rt("unsupported"), profile).kind).toBe("open");
    expect(runtimeShortcut(rt("installable"), profile).kind).toBe("open");
    expect(runtimeShortcut([], profile).kind).toBe("open");
    expect(runtimeShortcut(rt("stopped"), undefined).kind).toBe("open");
  });
  it("leaves the model out for a profile without one", () => {
    expect(runtimeShortcut(rt("stopped"), { ...profile, model: "" })).toEqual({ kind: "start", runtime: rt("stopped")[0], model: undefined });
  });
});

describe("downloadFraction", () => {
  it("is a share between 0 and 1", () => {
    expect(downloadFraction(50, 200)).toBe(0.25);
    expect(downloadFraction(300, 200)).toBe(1);
    expect(downloadFraction(5, 0)).toBe(0);
  });
});

describe("simple first run", () => {
  const rec = recommendedModel(catalog)!;
  const none = (): RuntimeInfo[] => runtimes.map((r) => ({ ...r, status: "unsupported" as const }));
  const local = (over: Partial<LocalModel> = {}): LocalModel => ({ ...models[0], ...over });
  const profile = { ...profiles[0], runtime_id: "llama-server" };
  const live = runtimes.map((r) => (r.id === "llama-server" ? { ...r, status: "running" as const } : r));

  it("offers the recommended model when nothing is installed", () => {
    const s = simpleFirstRun({ runtimes: live, models: [], profile, catalog });
    expect(s).toEqual({ kind: "prepare", model: rec });
  });
  it("waits for the catalog instead of guessing", () => {
    expect(simpleFirstRun({ runtimes: live, models: [], profile, catalog: undefined }).kind).toBe("loading");
  });
  it("says there is no model when the catalog has nothing that fits", () => {
    expect(simpleFirstRun({ runtimes: live, models: [], profile, catalog: catalog.map((c) => ({ ...c, installed: true })) }).kind).toBe("no-model");
  });
  it("sends to the developer view when no runtime can run a model", () => {
    expect(simpleFirstRun({ runtimes: none(), models: [], profile, catalog }).kind).toBe("no-runtime");
    const unrunnable: CatalogModel[] = catalog.map((c) => ({ ...c, runtime_compat: c.runtime_compat.map((r) => ({ ...r, available: false })) }));
    expect(simpleFirstRun({ runtimes: live, models: [], profile, catalog: unrunnable }).kind).toBe("no-runtime");
  });
  it("is just a chat when a model is installed and the profile can use it", () => {
    const m = local({ id: "m1" });
    expect(simpleFirstRun({ runtimes: live, models: [m], profile: { ...profile, model: "m1" }, catalog })).toEqual({ kind: "chat" });
  });
  it("connects an installed model when the default profile has none", () => {
    const m = local({ id: "m1", runtime_compat: [{ id: "llama-server", name: "llama.cpp", available: true, reason: "" }] });
    const s = simpleFirstRun({ runtimes: live, models: [m], profile: { ...profile, model: "" }, catalog });
    expect(s).toMatchObject({ kind: "connect", runtimeId: "llama-server" });
    expect(simpleFirstRun({ runtimes: live, models: [m], profile: undefined, catalog }).kind).toBe("connect");
  });
  it("has no usable profile when its model is gone or its runtime is down", () => {
    const m = local({ id: "m1" });
    expect(isUsableProfile({ ...profile, model: "other" }, [m], live)).toBe(false);
    expect(isUsableProfile({ ...profile, model: "m1" }, [m], none())).toBe(false);
    expect(isUsableProfile({ ...profile, model: "m1" }, [m], live)).toBe(true);
    expect(isUsableProfile(undefined, [m], live)).toBe(false);
  });
  it("stays on the overview until there is a chat ready and a first session", () => {
    expect(simpleFirstRunPending({ kind: "chat" }, 0)).toBe(true);
    expect(simpleFirstRunPending({ kind: "chat" }, 3)).toBe(false);
    expect(simpleFirstRunPending({ kind: "no-runtime" }, 3)).toBe(true);
  });
  it("picks the best installed chat model that something can run", () => {
    const compat = [{ id: "llama-server", name: "llama.cpp", available: true, reason: "" }];
    const small = local({ id: "small", size_bytes: 1, capabilities: ["chat", "tools"], fit: "great", runtime_compat: compat });
    const big = local({ id: "big", size_bytes: 9, capabilities: ["chat", "tools"], fit: "great", runtime_compat: compat });
    const tooBig = local({ id: "huge", size_bytes: 99, capabilities: ["chat", "tools"], fit: "no", runtime_compat: compat });
    const speech = local({ id: "whisper", size_bytes: 50, capabilities: ["stt"], fit: "great", runtime_compat: compat });
    const stuck = local({ id: "stuck", size_bytes: 70, capabilities: ["chat"], fit: "great", runtime_compat: [{ id: "x", name: "x", available: false, reason: "no" }] });
    expect(bestInstalledModel([small, big, tooBig, speech, stuck])).toEqual({ model: big, runtimeId: "llama-server" });
    expect(bestInstalledModel([speech, stuck])).toBeUndefined();
  });
  it("points the existing default profile at the model, or creates one called default", () => {
    const m = local({ id: "m1", context_length: 4096 });
    const next = profileForModel({ ...profile, params: { ...profile.params, context: 8192 } }, m, "llama-server");
    expect(next).toMatchObject({ id: profile.id, model: "m1", runtime_id: "llama-server" });
    expect(next.params.context).toBe(4096);
    expect(profileForModel(undefined, m, "llama-server")).toMatchObject({ id: "default", model: "m1", runtime_id: "llama-server" });
  });
});

describe("first run with the model adviser", () => {
  const choice = (over: Partial<ModelChoice> = {}): ModelChoice => ({ model: "m", name: "Modello", runtime_id: "llama-server", context: 8192, fit: "great", size_bytes: 1, reason: "r", ...over });
  const adv = (installed: ModelChoice | null, download: ModelChoice | null = null): TaskAdvice => ({ installed, download });
  const rec = (agent: TaskAdvice): Recommendations => ({ tier: "m", chat: adv(null), agent, reasoning: adv(null), vision: adv(null), embeddings: adv(null) });
  const live = runtimes.map((r) => (r.id === "llama-server" ? { ...r, status: "running" as const } : r));
  const profile = { ...profiles[0], runtime_id: "llama-server" };
  const free = catalog.filter((c) => c.downloadable && !c.installed && c.fit !== "no" && c.format === "gguf");

  it("offers the adviser's agent download instead of the old rule", () => {
    const pick = free[free.length - 1];
    const r = rec(adv(null, choice({ catalog_id: pick.id })));
    expect(firstRunModel(catalog, r)).toEqual(pick);
    expect(simpleFirstRun({ runtimes: live, models: [], profile, catalog, recommendation: r })).toEqual({ kind: "prepare", model: pick });
  });
  it("falls back to the old rule when the server has no adviser", () => {
    expect(firstRunModel(catalog, null)).toEqual(recommendedModel(catalog));
    expect(simpleFirstRun({ runtimes: live, models: [], profile, catalog, recommendation: null })).toEqual({ kind: "prepare", model: recommendedModel(catalog) });
  });
  it("trusts the adviser when it finds nothing that fits", () => {
    expect(simpleFirstRun({ runtimes: live, models: [], profile, catalog, recommendation: rec(adv(null)) }).kind).toBe("no-model");
  });
  it("waits while the adviser is being asked", () => {
    expect(simpleFirstRun({ runtimes: live, models: [], profile, catalog, recommendationPending: true }).kind).toBe("loading");
  });
  it("is ready to chat as soon as the model is installed when the agent follows the adviser", () => {
    const m: LocalModel = { ...models[0], id: "m1" };
    // The default profile does not point at it, yet nothing has to be patched.
    const stale = { ...profile, model: "" };
    expect(simpleFirstRun({ runtimes: live, models: [m], profile: stale, catalog, recommendation: rec(adv(choice())), followsAdviser: true })).toEqual({ kind: "chat" });
    // An agent on a stored profile (existing install) still goes through the patch path.
    expect(simpleFirstRun({ runtimes: live, models: [m], profile: stale, catalog, recommendation: rec(adv(choice())), followsAdviser: false }).kind).toBe("connect");
  });
  it("knows when the default agent follows the adviser", () => {
    const agent = (id: string, inference_profile: string): AgentProfile => ({ ...agents[0], id, inference_profile });
    const list = [agent("a", "default"), agent("b", "auto:agent")];
    expect(defaultAgent(list, "b")?.id).toBe("b");
    expect(followsAdviser(list, "b")).toBe(true);
    expect(followsAdviser(list, "a")).toBe(false);
    expect(followsAdviser(list, "missing")).toBe(false);
    expect(followsAdviser([], undefined)).toBe(false);
    // An auto profile is not in the stored list: the first stored profile stands in for the shortcuts.
    expect(defaultProfile(list, profiles, "b")).toEqual(profiles[0]);
  });
});
