import { describe, expect, it } from "vitest";
import { ApiError } from "../../shared/api/http";
import type { BrowserState } from "../../shared/api/types";
import {
  clickInput, clipDialogMessage, connectionText, controlsFor, dialogShape, fractionIn, frameAlt, frameDelay, hidesAfterAction, hidesBrowser, keyAction, MAX_DIALOG_CHARS, normalizeState, notes, pageLine,
  scrollInput, shortStatus, showsBrowser, splitText, statePollInterval, statusPhrase, wheelPixels, type KeyLike,
} from "./model";

const state = (s: Partial<BrowserState> = {}): BrowserState => ({
  running: true, headless: false, mode: "agent", agent_busy: false, stale: false, handover: null,
  page: { title: "Accedi", host: "example.com", secure: true }, dialog: null, focus: null, ...s,
});
const handover = { question_id: "ask_1", run_id: "run_1", reason: "Accedi" };

describe("normalizeState", () => {
  it("keeps a state of the contract as it is", () => {
    const full = state({ mode: "human", agent_busy: true, stale: true, handover, dialog: { kind: "prompt", message: "Nome?", default_text: "x" }, focus: "campo password «Chiave»" });
    expect(normalizeState(JSON.parse(JSON.stringify(full)))).toEqual(full);
  });
  it("gives the quiet value to what is missing or wrong: the agent drives, nothing waits", () => {
    expect(normalizeState({ running: true, mode: "robot", handover: { run_id: "r" }, page: "x", dialog: 3, focus: "" })).toEqual({
      running: true, headless: false, mode: "agent", agent_busy: false, stale: false, handover: null, page: null, dialog: null, focus: null,
    });
  });
  it("is null for what is not a state", () => {
    for (const v of [null, undefined, "x", 3, [], true]) expect(normalizeState(v)).toBeNull();
  });
});

describe("what hides the browser", () => {
  it("403 (a paired device) and 404 (an older backend) hide it; nothing else does", () => {
    expect(hidesBrowser(new ApiError(403, "no"))).toBe(true);
    expect(hidesBrowser(new ApiError(404, "no"))).toBe(true);
    for (const e of [new ApiError(409, "x"), new ApiError(500, "x"), new ApiError(401, "x"), new TypeError("Failed to fetch"), new Error("403"), null]) expect(hidesBrowser(e)).toBe(false);
  });
  it("a failed ACTION hides the browser only with a 403: a 404 or 409 there (the window was answered elsewhere, not your turn) leaves it, with its message", () => {
    expect(hidesAfterAction(new ApiError(403, "no"))).toBe(true);
    for (const e of [new ApiError(404, "nessuna finestra"), new ApiError(409, "prima prendi il controllo"), new ApiError(400, "x"), new ApiError(500, "x"), new TypeError("Failed to fetch"), null]) {
      expect(hidesAfterAction(e), String(e)).toBe(false);
    }
    // A read still hides it with a 404 (an older backend has no such route).
    expect(hidesBrowser(new ApiError(404, "no"))).toBe(true);
  });
  it("the poll stops once the browser is known to be unavailable, and goes on while it is known or not yet known", () => {
    expect(statePollInterval(null)).toBe(false);
    expect(statePollInterval(undefined)).toBe(2000);
    expect(statePollInterval(state())).toBe(2000);
  });
  it("the button and the panel exist while a browser runs or the agent asks for help", () => {
    expect(showsBrowser(null)).toBe(false);
    expect(showsBrowser(undefined)).toBe(false);
    expect(showsBrowser(state({ running: false }))).toBe(false);
    expect(showsBrowser(state())).toBe(true);
    expect(showsBrowser(state({ running: false, handover }))).toBe(true);
  });
});

describe("what the state says", () => {
  it("names who drives the browser", () => {
    expect(statusPhrase(state())).toBe("L'agente sta usando il browser");
    expect(statusPhrase(state({ mode: "paused" }))).toBe("Agente in pausa");
    expect(statusPhrase(state({ mode: "human" }))).toBe("Stai guidando il browser: l'agente aspetta");
    expect(statusPhrase(state({ running: false }))).toBe("Il browser non è aperto");
  });
  it("gives the header button a short state, and a request for help comes first", () => {
    expect(shortStatus(state())).toBe("l'agente lo sta usando");
    expect(shortStatus(state({ mode: "paused" }))).toBe("agente in pausa");
    expect(shortStatus(state({ mode: "human" }))).toBe("lo stai guidando tu");
    expect(shortStatus(state({ mode: "human", handover }))).toBe("serve il tuo intervento");
  });
  it("tells the host and the title of the page, and what the picture of it says", () => {
    expect(pageLine({ title: "Accedi", host: "example.com", secure: true })).toBe("example.com · Accedi");
    expect(pageLine({ title: "  ", host: "example.com", secure: true })).toBe("example.com");
    expect(pageLine(null)).toBe("Nessuna pagina aperta");
    expect(frameAlt({ title: "Accedi", host: "example.com", secure: true })).toBe("Pagina «Accedi» su example.com");
    expect(frameAlt({ title: "", host: "example.com", secure: false })).toBe("Pagina su example.com");
    expect(frameAlt(null)).toBe("Pagina del browser");
    expect(connectionText(true)).toContain("https");
    expect(connectionText(false)).toContain("non");
  });
});

describe("what each state allows", () => {
  it("while the agent drives: pause and take", () => {
    expect(controlsFor(state())).toEqual({ canPause: true, canResume: false, canTake: true, canGiveBack: false, canType: false, canAnswerDialog: false });
  });
  it("while paused: resume and take", () => {
    expect(controlsFor(state({ mode: "paused" }))).toEqual({ canPause: false, canResume: true, canTake: true, canGiveBack: false, canType: false, canAnswerDialog: false });
  });
  it("while the person drives: give back and type; no pause (the agent waits already)", () => {
    expect(controlsFor(state({ mode: "human" }))).toEqual({ canPause: false, canResume: false, canTake: false, canGiveBack: true, canType: true, canAnswerDialog: false });
  });
  it("typing waits for an action of the agent still running, and for a window of the site, which is answered instead", () => {
    expect(controlsFor(state({ mode: "human", agent_busy: true })).canType).toBe(false);
    const dialog = controlsFor(state({ mode: "human", dialog: { kind: "confirm", message: "Uscire?", default_text: "" } }));
    expect(dialog.canType).toBe(false);
    expect(dialog.canAnswerDialog).toBe(true);
    // The engine refuses the answer (409) while an action of the agent is still running, like an input: the buttons wait too.
    expect(controlsFor(state({ mode: "human", agent_busy: true, dialog: { kind: "confirm", message: "x", default_text: "" } })).canAnswerDialog).toBe(false);
    expect(controlsFor(state({ dialog: { kind: "confirm", message: "x", default_text: "" } })).canAnswerDialog).toBe(false);
  });
  it("without a browser nothing is allowed", () => {
    expect(Object.values(controlsFor(state({ running: false, mode: "human" }))).every((v) => v === false)).toBe(true);
  });
  it("explains why a control is off, and where else the browser can be seen", () => {
    const ids = (s: BrowserState) => notes(s).map((n) => n.id);
    expect(ids(state())).toEqual(["readonly", "headless"]);
    expect(ids(state({ mode: "human" }))).toEqual(["headless"]);
    expect(ids(state({ mode: "human", agent_busy: true }))).toEqual(["busy", "headless"]);
    expect(ids(state({ stale: true }))).toEqual(["stale", "readonly", "headless"]);
    expect(ids(state({ mode: "human", stale: true }))).toEqual(["headless"]);
    expect(ids(state({ mode: "human", dialog: { kind: "confirm", message: "x", default_text: "" } }))).toEqual(["window", "headless"]);
    // Busy and a window: «busy» says why the answer waits; «window» would point at a button that is off.
    expect(ids(state({ mode: "human", agent_busy: true, dialog: { kind: "confirm", message: "x", default_text: "" } }))).toEqual(["busy", "headless"]);
    expect(ids(state({ dialog: { kind: "confirm", message: "x", default_text: "" } }))).toEqual(["window", "headless"]);
    expect(ids(state({ running: false }))).toEqual([]);
    expect(notes(state()).find((n) => n.id === "headless")?.text).toBe("Puoi anche usare la finestra del browser sul computer.");
    expect(notes(state({ headless: true })).find((n) => n.id === "headless")?.text).toContain("l'unico modo");
  });
});

describe("the window of the site", () => {
  it("a prompt has a field, an alert has no way out but OK", () => {
    expect(dialogShape({ kind: "prompt" })).toMatchObject({ field: true, cancel: true });
    expect(dialogShape({ kind: "confirm" })).toMatchObject({ field: false, cancel: true });
    expect(dialogShape({ kind: "alert" })).toMatchObject({ field: false, cancel: false });
    expect(dialogShape({ kind: "beforeunload" })).toMatchObject({ field: false, cancel: true });
  });
  it("cuts a very long message of the site", () => {
    expect(clipDialogMessage("breve")).toBe("breve");
    const long = "x".repeat(MAX_DIALOG_CHARS + 50);
    expect(Array.from(clipDialogMessage(long))).toHaveLength(MAX_DIALOG_CHARS + 1);
    expect(clipDialogMessage(long).endsWith("…")).toBe(true);
  });
});

const key = (k: string, mods: Partial<KeyLike> = {}): KeyLike => ({ key: k, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, ...mods });

describe("keys of the viewer", () => {
  it("named keys go to the page, with Shift for the arrows (selecting text)", () => {
    for (const k of ["Enter", "Backspace", "Escape", "ArrowLeft", "ArrowUp", "ArrowRight", "ArrowDown", "Delete", "Home", "End", "PageUp", "PageDown"]) {
      expect(keyAction(key(k))).toEqual({ action: "send", input: { type: "key", key: k, shift: false } });
    }
    expect(keyAction(key("ArrowRight", { shiftKey: true }))).toEqual({ action: "send", input: { type: "key", key: "ArrowRight", shift: true } });
  });
  it("one printable character goes to the page, as it is typed (capital, space, symbol, emoji)", () => {
    expect(keyAction(key("a"))).toEqual({ action: "send", input: { type: "key", key: "a", shift: false } });
    expect(keyAction(key("A", { shiftKey: true }))).toEqual({ action: "send", input: { type: "key", key: "A", shift: true } });
    expect(keyAction(key(" "))).toEqual({ action: "send", input: { type: "key", key: " ", shift: false } });
    expect(keyAction(key("€"))).toMatchObject({ action: "send" });
    expect(keyAction(key("😀"))).toMatchObject({ action: "send", input: { key: "😀" } });
  });
  it("Tab and Shift+Tab leave the viewer: it is never a keyboard trap", () => {
    expect(keyAction(key("Tab"))).toEqual({ action: "pass" });
    expect(keyAction(key("Tab", { shiftKey: true }))).toEqual({ action: "pass" });
  });
  it("shortcuts stay with the browser: the engine takes none", () => {
    expect(keyAction(key("a", { ctrlKey: true }))).toEqual({ action: "pass" });
    expect(keyAction(key("v", { metaKey: true }))).toEqual({ action: "pass" });
    expect(keyAction(key("ArrowLeft", { metaKey: true }))).toEqual({ action: "pass" });
    expect(keyAction(key("Backspace", { ctrlKey: true }))).toEqual({ action: "pass" });
    expect(keyAction(key("ArrowLeft", { altKey: true }))).toEqual({ action: "pass" });
  });
  it("AltGr and Alt on a Mac keyboard make characters (@ # [ ]): they are the character, not a shortcut", () => {
    const altGr = { ctrlKey: true, altKey: true, getModifierState: (k: string) => k === "AltGraph" };
    expect(keyAction(key("@", altGr))).toEqual({ action: "send", input: { type: "key", key: "@", shift: false } });
    expect(keyAction(key("#", { altKey: true }))).toMatchObject({ action: "send", input: { key: "#" } });
  });
  it("what is not a key of text stays with the browser (function keys, dead keys, input methods)", () => {
    for (const k of ["F5", "Shift", "Control", "CapsLock", "Dead", "Unidentified", "Process", "ContextMenu"]) expect(keyAction(key(k))).toEqual({ action: "pass" });
    expect(keyAction(key("a", { isComposing: true }))).toEqual({ action: "pass" });
    expect(keyAction(key("Enter", { isComposing: true }))).toEqual({ action: "pass" });
  });
});

describe("clicks and wheel on the frame", () => {
  const box = { left: 100, top: 50, width: 400, height: 200 };
  it("a click is a fraction of the frame", () => {
    expect(fractionIn(box, 300, 150)).toEqual({ x: 0.5, y: 0.5 });
    expect(fractionIn(box, 100, 50)).toEqual({ x: 0, y: 0 });
    expect(fractionIn(box, 500, 250)).toEqual({ x: 1, y: 1 });
    expect(fractionIn(box, 125, 75)).toEqual({ x: 0.0625, y: 0.125 });
  });
  it("a point outside the frame is held at its edge, and a frame with no size gives nothing", () => {
    expect(fractionIn(box, 0, 1000)).toEqual({ x: 0, y: 1 });
    expect(fractionIn({ ...box, width: 0 }, 10, 10)).toBeNull();
    expect(fractionIn({ ...box, height: Number.NaN }, 10, 10)).toBeNull();
  });
  it("the click carries the left button and the count of the browser's click (a double click is two, the second counts 2)", () => {
    expect(clickInput(box, { clientX: 300, clientY: 150 })).toEqual({ type: "click", x: 0.5, y: 0.5, button: "left", count: 1 });
    expect(clickInput(box, { clientX: 300, clientY: 150, detail: 2 })).toMatchObject({ count: 2 });
    expect(clickInput(box, { clientX: 300, clientY: 150, detail: 9 })).toMatchObject({ count: 3 });
    expect(clickInput(box, { clientX: 300, clientY: 150, detail: 0 })).toMatchObject({ count: 1 });
    expect(clickInput({ ...box, width: 0 }, { clientX: 1, clientY: 1 })).toBeNull();
  });
  it("the wheel is turned into pixels of the page, within what one request may ask", () => {
    expect(wheelPixels(120, 0, 500)).toBe(120);
    expect(wheelPixels(3, 1, 500)).toBe(48);
    expect(wheelPixels(1, 2, 500)).toBe(500);
    expect(wheelPixels(-1, 2, 40)).toBe(-100);
    expect(wheelPixels(100_000, 0, 500)).toBe(1500);
    expect(wheelPixels(-100_000, 0, 500)).toBe(-1500);
    expect(wheelPixels(0.4, 0, 500)).toBe(0);
  });
  it("a scroll names where it is and how far, and nothing is sent for no distance", () => {
    expect(scrollInput(box, 300, 150, 120)).toEqual({ type: "scroll", x: 0.5, y: 0.5, dy: 120 });
    expect(scrollInput(box, 300, 150, 0)).toBeNull();
    expect(scrollInput({ ...box, height: 0 }, 300, 150, 5)).toBeNull();
  });
});

describe("splitText", () => {
  it("cuts a long paste into pieces the engine accepts (512 characters)", () => {
    expect(splitText("")).toEqual([]);
    expect(splitText("codice")).toEqual(["codice"]);
    expect(splitText("a".repeat(512))).toHaveLength(1);
    expect(splitText("a".repeat(513)).map((p) => p.length)).toEqual([512, 1]);
    expect(splitText("a".repeat(1300)).map((p) => p.length)).toEqual([512, 512, 276]);
  });
  it("counts characters, not UTF-16 units: 512 emoji are one piece, and no piece cuts one in two", () => {
    expect(splitText("😀".repeat(512))).toHaveLength(1);
    const parts = splitText("😀".repeat(1000));
    expect(parts.map((p) => Array.from(p).length)).toEqual([512, 488]);
    expect(parts.join("")).toBe("😀".repeat(1000));
  });
  it("sends line ends as one newline", () => {
    expect(splitText("a\r\nb\rc")).toEqual(["a\nb\nc"]);
  });
});

describe("how often to look at the page", () => {
  const f = (o: Partial<Parameters<typeof frameDelay>[0]> = {}) => frameDelay({ running: true, mode: "agent", agentBusy: false, hidden: false, dialog: false, ...o });
  it("about twice a second while the person drives, slower while the agent works or waits", () => {
    expect(f({ mode: "human" })).toBe(500);
    expect(f({ mode: "agent", agentBusy: true })).toBe(1500);
    expect(f({ mode: "agent" })).toBe(1000);
    expect(f({ mode: "paused" })).toBe(1000);
    expect(f({ mode: "human", agentBusy: true })).toBe(1000);
  });
  it("not at all while a window of the site waits (the engine answers 409 to a frame: the last one stays), and again once it is gone", () => {
    expect(f({ dialog: true })).toBeNull();
    expect(f({ dialog: true, mode: "human" })).toBeNull();
    expect(f({ dialog: false, mode: "human" })).toBe(500);
  });
  it("not at all while the tab is hidden or no browser runs", () => {
    expect(f({ hidden: true })).toBeNull();
    expect(f({ hidden: true, mode: "human" })).toBeNull();
    expect(f({ running: false })).toBeNull();
  });
});
