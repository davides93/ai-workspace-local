import { describe, expect, it } from "vitest";
import { catalog, models, profiles, agents, runtimes } from "../../../shared/api/mock/fixtures";
import { checklist, defaultProfile, needsFirstRun, recommendedModel } from "./overview";

describe("first-run checklist", () => {
  it("is not needed with a model and a usable runtime", () => {
    expect(needsFirstRun(models, runtimes)).toBe(false);
  });
  it("is needed without local models", () => {
    expect(needsFirstRun([], runtimes)).toBe(true);
  });
  it("is needed without a running or available runtime", () => {
    expect(needsFirstRun(models, runtimes.map((r) => ({ ...r, status: r.status === "running" || r.status === "available" ? "stopped" : r.status })))).toBe(true);
  });
  it("marks the steps that are done", () => {
    const steps = checklist({ runtimes, models: [], profiles, sessions: 0 });
    expect(steps.map((s) => [s.id, s.done])).toEqual([["runtime", true], ["model", false], ["profile", true], ["chat", false]]);
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
