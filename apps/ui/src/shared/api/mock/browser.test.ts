import { describe, expect, it } from "vitest";
import { ApiError } from "../http";
import type { AgEvent, BrowserState, Question } from "../types";
import { MockTransport } from "./index";
import { BrowserDemo } from "./browser";

const noAnswer = () => {};

const frameText = (b: BrowserDemo): string => {
  const f = b.frame();
  if (!f.running) throw new Error("no browser");
  return decodeURIComponent(f.image.replace(/^data:image\/svg\+xml;utf8,/, ""));
};

describe("the browser of the demo", () => {
  it("is closed until the agent opens a page, and a closed browser has no frame", () => {
    const b = new BrowserDemo();
    expect(b.state()).toMatchObject({ running: false, mode: "agent", page: null, handover: null, dialog: null, focus: null });
    expect(b.frame()).toEqual({ running: false });
    b.open("quotes");
    expect(b.state()).toMatchObject({ running: true, mode: "agent", page: { host: "fornitori.example.com", secure: true } });
    const f = b.frame();
    expect(f).toMatchObject({ running: true, width: 1000, height: 500 });
    expect(f.running && f.image.startsWith("data:image/svg+xml")).toBe(true);
  });

  it("pause, resume, take and give back follow the rules of the engine", () => {
    const b = new BrowserDemo();
    b.open("quotes");
    expect(b.control("pause", noAnswer).mode).toBe("paused");
    expect(b.control("resume", noAnswer).mode).toBe("agent");
    expect(b.control("take", noAnswer).mode).toBe("human");
    // The browser is in the person's hands: resuming the agent is refused, with the way out.
    expect(() => b.control("resume", noAnswer)).toThrowError(/Restituisci all'agente/);
    // Pausing the agent that already waits changes nothing.
    expect(b.control("pause", noAnswer).mode).toBe("human");
    const back = b.control("give_back", noAnswer);
    expect(back).toMatchObject({ mode: "agent", stale: true, handover: null });
    expect(() => b.control("explode" as never, noAnswer)).toThrowError(ApiError);
  });

  it("input is refused unless the person drives and nothing of the agent is running, with the reason in Italian", () => {
    const b = new BrowserDemo();
    b.open("login");
    expect(() => b.input({ type: "text", text: "x" })).toThrowError(/prima prendi il controllo/);
    b.control("take", noAnswer);
    b.setBusy(true);
    expect(() => b.input({ type: "text", text: "x" })).toThrowError(/l'agente sta finendo un'azione/);
    b.setBusy(false);
    expect(() => b.input({ type: "text", text: "x".repeat(513) })).toThrowError(/512/);
    expect(() => b.input({ type: "text", text: "x" })).not.toThrow();
  });

  it("says in words which control has the focus, only while the person drives, and never its value", () => {
    const b = new BrowserDemo();
    b.open("login");
    expect(b.state().focus).toBeNull();
    b.control("take", noAnswer);
    expect(b.state().focus).toBeNull();
    b.input({ type: "key", key: "Tab", shift: false });
    expect(b.state().focus).toBe("campo di testo «Email»");
    b.input({ type: "key", key: "Tab", shift: false });
    expect(b.state().focus).toBe("campo password «Password»");
    b.input({ type: "key", key: "Tab", shift: true });
    expect(b.state().focus).toBe("campo di testo «Email»");
    b.control("give_back", noAnswer);
    expect(b.state().focus).toBeNull();
  });

  it("keeps no value: typing changes the frame (one dot a character) and the value is nowhere", () => {
    const b = new BrowserDemo();
    b.open("login");
    b.control("take", noAnswer);
    const before = frameText(b);
    b.input({ type: "click", x: 0.5, y: 0.55, button: "left", count: 1 });
    expect(b.state().focus).toBe("campo password «Password»");
    b.input({ type: "text", text: "hunter2-TEST-ONLY" });
    b.input({ type: "key", key: "x", shift: false });
    const after = frameText(b);
    expect(after).not.toBe(before);
    expect(after).toContain("•".repeat(18));
    expect(after).not.toContain("hunter2");
    expect(JSON.stringify(b)).not.toContain("hunter2");
  });

  it("signs in when both fields have something, then a click on «Esci» asks first (a window of the site that blocks the frame until answered)", () => {
    const b = new BrowserDemo();
    b.open("login");
    b.control("take", noAnswer);
    b.input({ type: "click", x: 0.5, y: 0.35, button: "left", count: 1 });
    b.input({ type: "text", text: "anna@example.com" });
    b.input({ type: "key", key: "Enter", shift: false });
    expect(b.state().page?.title).toBe("Accedi · Fornitori");
    b.input({ type: "key", key: "Tab", shift: false });
    b.input({ type: "text", text: "pw" });
    b.input({ type: "key", key: "Enter", shift: false });
    expect(b.state().page?.title).toBe("Area riservata · Fornitori");

    b.input({ type: "click", x: 0.9, y: 0.9, button: "left", count: 1 });
    expect(b.state().dialog).toEqual({ kind: "confirm", message: "Vuoi davvero uscire dall'area riservata?", default_text: "" });
    expect(() => b.frame()).toThrowError(/finestra del sito/);
    expect(() => b.input({ type: "key", key: "a", shift: false })).toThrowError(/finestra del sito/);
    b.answerDialog({ action: "dismiss" });
    expect(b.state().dialog).toBeNull();
    expect(b.state().page?.title).toBe("Area riservata · Fornitori");
    b.input({ type: "click", x: 0.9, y: 0.9, button: "left", count: 1 });
    b.answerDialog({ action: "accept" });
    expect(b.state().page?.title).toBe("Accedi · Fornitori");
    // A second answer to the same window: 409, like the engine (not a 404 that would read as «this browser does not exist»).
    expect(() => b.answerDialog({ action: "accept" })).toThrowError(/nessuna finestra/);
    try {
      b.answerDialog({ action: "accept" });
    } catch (e) {
      expect(e).toMatchObject({ status: 409 });
    }
  });

  it("a window of the site is answered only while the person drives and no action of the agent runs (the engine's `human_begin`), always with 409", () => {
    const b = new BrowserDemo();
    b.open("area");
    const refused = (fn: () => void) => {
      try {
        fn();
      } catch (e) {
        return e;
      }
      return null;
    };
    expect(refused(() => b.answerDialog({ action: "accept" }))).toMatchObject({ status: 409, message: "prima prendi il controllo del browser" });
    b.control("take", noAnswer);
    b.input({ type: "click", x: 0.9, y: 0.9, button: "left", count: 1 });
    expect(b.state().dialog).not.toBeNull();
    b.setBusy(true);
    expect(refused(() => b.answerDialog({ action: "accept" }))).toMatchObject({ status: 409, message: expect.stringContaining("l'agente sta finendo un'azione") });
    expect(b.state().dialog).not.toBeNull();
    b.setBusy(false);
    expect(() => b.answerDialog({ action: "dismiss" })).not.toThrow();
  });

  it("a handover gives the browser to the person at once and give_back answers its question", () => {
    const b = new BrowserDemo();
    b.open("login");
    b.beginHandover({ question_id: "ask_1", run_id: "run_1", reason: "Accedi" });
    expect(b.state()).toMatchObject({ mode: "human", handover: { question_id: "ask_1", reason: "Accedi" } });
    const answered: string[] = [];
    const s = b.control("give_back", (id) => answered.push(id));
    expect(answered).toEqual(["ask_1"]);
    expect(s).toMatchObject({ mode: "agent", handover: null, stale: true });
    // A run that ends or is stopped gives it back too.
    b.beginHandover({ question_id: "ask_2", run_id: "run_2", reason: "x" });
    b.releaseRun("run_other");
    expect(b.state().mode).toBe("human");
    b.releaseRun("run_2");
    expect(b.state()).toMatchObject({ mode: "agent", handover: null });
  });
});

type Custom = Extract<AgEvent, { type: "CUSTOM" }>;

function waitFor<T extends AgEvent>(api: MockTransport, runId: string, fits: (e: AgEvent) => e is T): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("event not seen")), 8000);
    let done = false;
    let off: (() => void) | undefined;
    const stop = api.subscribeRun(runId, (e) => {
      if (done || !fits(e)) return;
      done = true;
      clearTimeout(timer);
      queueMicrotask(() => off?.());
      resolve(e);
    });
    off = stop;
    if (done) queueMicrotask(stop);
  });
}
const custom = (name: string) => (e: AgEvent): e is Custom => e.type === "CUSTOM" && e.name === name;
const finished = (e: AgEvent): e is Extract<AgEvent, { type: "RUN_FINISHED" }> => e.type === "RUN_FINISHED";

async function start(api: MockTransport, text: string) {
  const session = await api.createSession({ agent_id: "operator" });
  const { run_id } = await api.startRun(session.id, text);
  return { session, runId: run_id };
}

describe("the demo of the live view (MockTransport)", () => {
  it("has no browser until a run opens one", async () => {
    const api = new MockTransport();
    expect(await api.browserState()).toMatchObject({ running: false });
    expect(await api.browserFrame()).toEqual({ running: false });
  });

  it("the script «Accedi al sito dei fornitori» asks for help: a handover question, the browser in the person's hands, and «give_back» ends both", async () => {
    const api = new MockTransport();
    const { runId } = await start(api, "Accedi al sito dei fornitori");
    const asked = await waitFor(api, runId, custom("aiw.question"));
    const q = asked.value as Question;
    expect(q).toMatchObject({ kind: "handover", confirm_label: "Ho finito: restituisci il controllo", deny_label: "Non posso ora" });
    expect(q.message).toContain("fornitori.example.com");
    expect((await api.questions()).map((x) => x.id)).toEqual([q.id]);

    const during = await api.browserState();
    expect(during).toMatchObject({ running: true, mode: "human", handover: { question_id: q.id, run_id: runId } });
    expect((await api.browserFrame()).running).toBe(true);

    // The text goes to the page; nothing of it is anywhere afterwards.
    await api.browserInput({ type: "text", text: "TEST-ONLY-secret-code-424242" });
    const back = await api.browserControl("give_back");
    expect(back).toMatchObject({ mode: "agent", handover: null, stale: true });
    const resolved = await waitFor(api, runId, custom("aiw.question_resolved"));
    expect(resolved.value).toMatchObject({ id: q.id, outcome: "answered", answer: { confirmed: true } });
    await waitFor(api, runId, finished);
    expect(await api.questions()).toEqual([]);
    const sessions = await api.sessions();
    const everything = JSON.stringify([await Promise.all(sessions.map((x) => api.messages(x.id))), await api.audit(), await api.browserState(), await api.questions()]);
    expect(everything).not.toContain("TEST-ONLY-secret-code");
  });

  it("answering the question from the chat card gives the browser back too, and «Non posso ora» does as well", async () => {
    const api = new MockTransport();
    const { runId } = await start(api, "Accedi al sito dei fornitori");
    const q = (await waitFor(api, runId, custom("aiw.question"))).value as Question;
    await api.answerQuestion(q.id, { action: "decline" });
    await waitFor(api, runId, finished);
    const s: BrowserState = await api.browserState();
    expect(s).toMatchObject({ mode: "agent", handover: null });
  });

  it("stopping the run while it waits for the person gives the browser back", async () => {
    const api = new MockTransport();
    const { runId } = await start(api, "Accedi al sito dei fornitori");
    await waitFor(api, runId, custom("aiw.question"));
    await api.cancelRun(runId);
    expect(await api.browserState()).toMatchObject({ mode: "agent", handover: null });
  });

  it("a paired device (`?scope=device`) gets a 403 on every route of the browser", async () => {
    const api = new MockTransport();
    api.secretScope = "device";
    for (const call of [() => api.browserState(), () => api.browserFrame(), () => api.browserControl("pause"), () => api.browserInput({ type: "text", text: "x" }), () => api.browserDialog({ action: "accept" })]) {
      await expect(call()).rejects.toMatchObject({ status: 403 });
    }
  });

  it("input before taking control is a 409 with the engine's sentence", async () => {
    const api = new MockTransport();
    await expect(api.browserInput({ type: "text", text: "x" })).rejects.toMatchObject({ status: 409, message: "prima prendi il controllo del browser" });
  });

  it("a phone looks only after the computer says yes, and the computer can end it", async () => {
    const api = new MockTransport();
    await expect(api.browserViewFrame()).rejects.toMatchObject({ status: 403 });
    expect(await api.browserViewRequest()).toMatchObject({ state: "pending" });
    await expect(api.browserViewFrame()).rejects.toMatchObject({ status: 403 });
    const { requests } = await api.browserViewRequests();
    expect(requests).toHaveLength(1);
    await api.browserViewDecide(requests[0].id, true);
    expect(await api.browserViewStatus()).toMatchObject({ state: "granted" });
    await expect(api.browserViewFrame()).resolves.toBeDefined(); // the demo browser is not open without a task: `{ running: false }`, not a 403
    expect((await api.browserViewRequests()).viewers).toHaveLength(1);
    await api.browserViewRevoke();
    await expect(api.browserViewFrame()).rejects.toMatchObject({ status: 403 });
  });

  it("a phone cannot answer its own request", async () => {
    const api = new MockTransport();
    await api.browserViewRequest();
    api.secretScope = "device";
    await expect(api.browserViewRequests()).rejects.toMatchObject({ status: 403 });
    await expect(api.browserViewDecide("vw_demo", true)).rejects.toMatchObject({ status: 403 });
    await expect(api.browserViewRevoke()).rejects.toMatchObject({ status: 403 });
  });
});
