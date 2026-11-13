import { describe, expect, it } from "vitest";
import { catalog, models, runtimes, sessions } from "./fixtures";
import { MockTransport } from "./index";
import { applyMockScenario, isMockScenario } from "./scenario";

describe("mock scenarios", () => {
  it("knows its names", () => {
    expect(isMockScenario("first-run")).toBe(true);
    expect(isMockScenario("nope")).toBe(false);
    expect(isMockScenario(null)).toBe(false);
  });

  it("first-run: a runtime that is up, no model, no conversation, and the adviser offers a download", async () => {
    applyMockScenario("first-run");
    expect(models).toHaveLength(0);
    expect(sessions).toHaveLength(0);
    expect(catalog.every((c) => !c.installed)).toBe(true);
    expect(runtimes.some((r) => r.status === "running")).toBe(true);
    const api = new MockTransport();
    expect(await api.models()).toEqual([]);
    expect(await api.sessions()).toEqual([]);
    const rec = await api.recommend();
    expect(rec.agent.installed).toBeNull();
    expect(rec.agent.download).not.toBeNull();
  });
});
