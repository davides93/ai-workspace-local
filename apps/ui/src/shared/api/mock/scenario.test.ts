import { describe, expect, it } from "vitest";
import { catalog, models, runtimes, sessions } from "./fixtures";
import { MockTransport } from "./index";
import { applyMockScenario, isMockScenario } from "./scenario";

describe("mock scenarios", () => {
  it("knows its names", () => {
    expect(isMockScenario("first-run")).toBe(true);
    expect(isMockScenario("mas")).toBe(true);
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

  it("mas: the sandboxed Mac App Store build, with a folder already granted that can be added to and taken off", async () => {
    const api = new MockTransport();
    // The default demo has no folders to grant.
    expect(await api.folders()).toEqual({ supported: false, folders: [] });
    await expect(api.addFolders("m", "p")).rejects.toThrow(/una cartella sola/);
    applyMockScenario("mas");
    const sys = await api.system();
    expect(sys.sandboxed).toBe(true);
    expect(sys.folder_access).toBe(true);
    const first = await api.folders();
    expect(first.supported).toBe(true);
    expect(first.folders.map((f) => f.name)).toEqual(["Download"]);
    const added = await api.addFolders("m", "p");
    expect(added.added).toBe(1);
    expect(added.folders.map((f) => f.name)).toEqual(["Download", "Documenti"]);
    // The demo has two more folders to offer; after that the panel adds nothing.
    await api.addFolders("m", "p");
    expect((await api.addFolders("m", "p")).added).toBe(0);
    const left = await api.removeFolder(added.folders[0]!.id);
    expect(left.folders.map((f) => f.name)).toEqual(["Documenti", "Progetti"]);
    await expect(api.removeFolder("fld_nope")).rejects.toThrow(/non trovata/);
  });
});
