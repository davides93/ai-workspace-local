import { describe, expect, it } from "vitest";
import { catalog, models, profiles, agents, runtimes } from "../../../shared/api/mock/fixtures";
import { checklist, currentStep, defaultProfile, downloadFraction, firstRunPending, isStartableRuntime, isUsableRuntime, recommendedModel, runtimeShortcut } from "./overview";

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
