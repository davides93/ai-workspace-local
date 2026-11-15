import { describe, expect, it } from "vitest";
import { agents, catalog, models, runtimes, sessions } from "./fixtures";
import { MockTransport } from "./index";
import { grammarFor } from "../../ui/highlight/languages";
import { LONG_CHAT_FIRST, LONG_CHAT_ID, LONG_CHAT_LAST, LONG_CHAT_NEEDLE, LONG_CHAT_TURNS, longChatMessages } from "./long-chat";
import { DEMO_PAIRING_CODE, applyMockScenario, applyMockUiMode, isMockScenario, transportForScenario } from "./scenario";

describe("mock scenarios", () => {
  it("knows its names", () => {
    expect(isMockScenario("first-run")).toBe(true);
    expect(isMockScenario("mas")).toBe(true);
    expect(isMockScenario("long-chat")).toBe(true);
    expect(isMockScenario("offline")).toBe(true);
    expect(isMockScenario("unpaired")).toBe(true);
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

  it("long-chat: one very long conversation, built the same way every time, with known words in it", async () => {
    applyMockScenario("long-chat");
    const api = new MockTransport();
    expect((await api.sessions())[0].id).toBe(LONG_CHAT_ID);
    const msgs = await api.messages(LONG_CHAT_ID);
    // question, tool call, tool result, answer: four messages per turn.
    expect(msgs).toHaveLength(LONG_CHAT_TURNS * 4);
    const text = msgs.map((m) => m.content).join("\n");
    expect(text).toContain(LONG_CHAT_FIRST);
    expect(text).toContain(LONG_CHAT_LAST);
    expect(text.split(LONG_CHAT_NEEDLE)).toHaveLength(2);
    // Every fenced block is closed, and the languages are the ones the highlighter knows.
    const fences = [...text.matchAll(/```(\w*)\n[\s\S]*?\n```/g)].map((m) => m[1]);
    expect(fences).toHaveLength(LONG_CHAT_TURNS);
    expect(new Set(fences)).toEqual(new Set(["ts", "python", "rust", "bash", "json", "sql", "css", "diff", "yaml", "go"]));
    expect(fences.every((l) => grammarFor(l) !== undefined)).toBe(true);
    // Nothing is random: two builds give the same text.
    const fixed = () => "2026-11-15T00:00:00.000Z";
    expect(longChatMessages(5, fixed)).toEqual(longChatMessages(5, fixed));
  });
});

describe("mock interface mode", () => {
  it("starts in simple mode and `?ui=developer` makes the configuration say developer", async () => {
    const api = new MockTransport();
    expect((await api.config()).effective["ui.mode"]).toBe("simple");
    applyMockUiMode("developer");
    expect((await api.config()).effective["ui.mode"]).toBe("developer");
    // The person can still change it afterwards, like any other value.
    expect((await api.setConfig("global", "ui.mode", "simple")).effective["ui.mode"]).toBe("simple");
  });
});

describe("mock agents", () => {
  it("has the Assistente (the agent of the simple interface) after the operator, which stays the first one", async () => {
    const api = new MockTransport();
    const list = await api.agents();
    expect(list[0].id).toBe("operator");
    const assistant = list.find((a) => a.id === "assistant");
    expect(assistant).toMatchObject({ name: "Assistente", inference_profile: "auto:agent", mode: "ask", sandbox: "workspace-write", max_turns: 40, delegate_tools: ["*"] });
    expect(assistant?.tools).toContain("ask_user");
    expect(assistant?.tools).toContain("aiw_secret_request");
    expect(agents).toHaveLength(7);
  });
});

describe("connection scenarios", () => {
  it("the data scenarios leave the transport as it is", () => {
    const demo = new MockTransport();
    expect(transportForScenario(demo, null)).toBe(demo);
    expect(transportForScenario(demo, "first-run")).toBe(demo);
  });

  it("offline: every request fails with a 503, and subscribing to a run (not a request) still works", async () => {
    const api = transportForScenario(new MockTransport(), "offline");
    await expect(api.system()).rejects.toMatchObject({ status: 503 });
    await expect(api.models()).rejects.toMatchObject({ status: 503 });
    await expect(api.sessions()).rejects.toMatchObject({ status: 503 });
    expect(api.kind).toBe("mock");
    expect(api.capabilities.demo).toBe(true);
    expect(typeof api.subscribeRun("run_x", () => {})).toBe("function");
  });

  it("unpaired: every request fails with a 401 until the right code is given, a wrong or empty one is refused, and then it all works", async () => {
    const api = transportForScenario(new MockTransport(), "unpaired");
    expect(api.capabilities.pairing).toBe(true);
    await expect(api.system()).rejects.toMatchObject({ status: 401 });
    await expect(api.pair!("000000")).rejects.toMatchObject({ status: 400 });
    await expect(api.pair!("")).rejects.toMatchObject({ status: 400 });
    await expect(api.models()).rejects.toMatchObject({ status: 401 });
    // The code may be typed with a space in the middle, as it is shown.
    await api.pair!(`${DEMO_PAIRING_CODE.slice(0, 3)} ${DEMO_PAIRING_CODE.slice(3)}`);
    expect((await api.system()).version).toBeTruthy();
    expect(Array.isArray(await api.models())).toBe(true);
  });
});
