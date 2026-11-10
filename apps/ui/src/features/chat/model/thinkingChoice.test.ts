import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/** A storage that lives in a Map; the module is loaded again for each test so its memory starts empty. */
function fakeStorage(data = new Map<string, string>()) {
  return {
    data,
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
    removeItem: (k: string) => void data.delete(k),
  };
}
const load = async () => {
  vi.resetModules();
  return import("./thinkingChoice");
};

describe("Ragionamento chosen in a conversation", () => {
  let storage = fakeStorage();
  beforeEach(() => {
    storage = fakeStorage();
    vi.stubGlobal("localStorage", storage);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("is remembered per session", async () => {
    const m = await load();
    expect(m.getThinkingChoice("s1")).toBeUndefined();
    m.setThinkingChoice("s1", "deep");
    m.setThinkingChoice("s2", "off");
    expect(m.getThinkingChoice("s1")).toBe("deep");
    expect(m.getThinkingChoice("s2")).toBe("off");
  });
  it("survives a reload of the page (read back from the storage)", async () => {
    (await load()).setThinkingChoice("s1", "off");
    expect((await load()).getThinkingChoice("s1")).toBe("off");
  });
  it("ignores a stored value that is not a mode, and no session", async () => {
    storage.setItem("aiw.chat.thinking.s1", "tanto");
    const m = await load();
    expect(m.getThinkingChoice("s1")).toBeUndefined();
    expect(m.getThinkingChoice(null)).toBeUndefined();
    expect(() => m.setThinkingChoice(null, "auto")).not.toThrow();
  });
  it("is forgotten with the session", async () => {
    const m = await load();
    m.setThinkingChoice("s1", "deep");
    m.forgetThinkingChoice("s1");
    expect(m.getThinkingChoice("s1")).toBeUndefined();
    expect(storage.data.has("aiw.chat.thinking.s1")).toBe(false);
  });
  it("still works for this page when the storage throws", async () => {
    vi.stubGlobal("localStorage", { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); }, removeItem: () => { throw new Error("blocked"); } });
    const m = await load();
    expect(() => m.setThinkingChoice("s1", "deep")).not.toThrow();
    expect(m.getThinkingChoice("s1")).toBe("deep");
    expect(m.getThinkingChoice("s2")).toBeUndefined();
  });
});
