import type { BrowserDialog, BrowserInput, BrowserMode, BrowserPage, BrowserState } from "../../shared/api/types";
import { isForbidden, isNotFound } from "../../shared/lib/errors";

import { t } from "../../shared/i18n";

// The decisions of the live view of the browser, apart from the screen: what each state says, which controls it allows, how a key or a
// click of the person becomes a request, and when to ask the engine again. All pure, so that the unit tests can say what the screen does.

/** The state of the browser is asked again this often while the chat is open. */
export const STATE_POLL_MS = 2_000;
/** Longest text the engine accepts in one `text` input (characters, not UTF-16 units). */
export const MAX_TEXT_CHARS = 512;
/** Longest message of a window of the site that is shown whole: it is the site's text, not ours. */
export const MAX_DIALOG_CHARS = 600;

const MODES: readonly BrowserMode[] = ["agent", "paused", "human"];
const isRecord = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const text = (v: unknown, fallback = ""): string => (typeof v === "string" ? v : fallback);

/**
 * The state as the engine sent it, made safe to use: a missing field takes the quiet value (the agent drives, nothing waits).
 * `null` when what came is not a state at all.
 */
export function normalizeState(v: unknown): BrowserState | null {
  if (!isRecord(v)) return null;
  const mode = MODES.includes(v.mode as BrowserMode) ? (v.mode as BrowserMode) : "agent";
  const page = isRecord(v.page) ? { title: text(v.page.title), host: text(v.page.host), secure: v.page.secure === true } : null;
  const dialog = isRecord(v.dialog) ? { kind: text(v.dialog.kind, "confirm"), message: text(v.dialog.message), default_text: text(v.dialog.default_text) } : null;
  const handover = isRecord(v.handover) && typeof v.handover.question_id === "string"
    ? { question_id: v.handover.question_id, run_id: text(v.handover.run_id), reason: text(v.handover.reason) }
    : null;
  return {
    running: v.running === true, headless: v.headless === true, mode, agent_busy: v.agent_busy === true, stale: v.stale === true,
    handover, page, dialog, focus: typeof v.focus === "string" && v.focus !== "" ? v.focus : null,
  };
}

/**
 * Does this failure of a READ (`GET /api/browser`, `GET /api/browser/frame`) mean «hide everything, quietly»? 403: a paired device may not
 * see the browser; 404: a backend that does not have the routes yet. Neither is asked again.
 */
export const hidesBrowser = (e: unknown): boolean => isForbidden(e) || isNotFound(e);

/**
 * The same for an ACTION (control, input, the answer to a window of the site). Only a 403 hides the browser: a 404 or a 409 there says the
 * thing asked is not (or no longer) there — the window was answered elsewhere, it is not the person's turn — and the state is asked again
 * with the engine's sentence, not the whole view taken away.
 */
export const hidesAfterAction = (e: unknown): boolean => isForbidden(e);

/** `false` (stop) once the browser is known to be unavailable (`null`), else the poll interval. */
export const statePollInterval = (data: BrowserState | null | undefined): number | false => (data === null ? false : STATE_POLL_MS);

/** The button of the chat header and the panel exist while a browser runs or the agent asks for help. */
export const showsBrowser = (s: BrowserState | null | undefined): s is BrowserState => !!s && (s.running || s.handover !== null);

// ---------------------------------------------------------------- what the state says

/** The sentence of the status line: who drives the browser. */
export function statusPhrase(s: Pick<BrowserState, "running" | "mode">): string {
  if (!s.running) return t("browser.non_aperto");
  switch (s.mode) {
    case "paused": return t("browser.stato_pausa");
    case "human": return t("browser.stato_utente");
    default: return t("browser.stato_agente");
  }
}

/** A word or two for the header button (read after «Browser»). */
export function shortStatus(s: Pick<BrowserState, "mode" | "handover">): string {
  if (s.handover) return t("browser.stato_breve_serve");
  switch (s.mode) {
    case "paused": return t("browser.stato_breve_pausa");
    case "human": return t("browser.stato_breve_utente");
    default: return t("browser.stato_breve_agente");
  }
}

/** Where the person is: the host, then the title of the page; the title can be empty. */
export function pageLine(p: BrowserPage | null): string {
  if (!p) return t("browser.nessuna_pagina");
  return p.title.trim() ? `${p.host} · ${p.title.trim()}` : p.host;
}

/** The text alternative of the frame: the title and the host, like the sighted person reads them in the bar. */
export function frameAlt(p: BrowserPage | null): string {
  if (!p) return t("browser.alt_nessuna");
  return p.title.trim() ? t("browser.alt_pagina", { title: p.title.trim(), host: p.host }) : t("browser.alt_pagina_senza_titolo", { host: p.host });
}

export const connectionText = (secure: boolean): string => (secure ? t("browser.sicura") : t("browser.non_sicura"));

/** A line the panel shows besides the status, and why. */
export interface Note {
  id: "stale" | "busy" | "window" | "headless" | "readonly";
  text: string;
}

/** What the state asks the person to know: why a control is off, what the agent will do, where else the browser can be seen. */
export function notes(s: BrowserState): Note[] {
  if (!s.running) return [];
  const out: Note[] = [];
  if (s.mode === "human" && s.agent_busy) out.push({ id: "busy", text: t("browser.nota_occupato") });
  // While the agent still runs an action the window cannot be answered yet: «busy» says so, and «window» would send the person to a button that is off.
  if (s.dialog && !(s.mode === "human" && s.agent_busy)) out.push({ id: "window", text: s.mode === "human" ? t("browser.nota_finestra_blocca") : t("browser.nota_finestra_agente") });
  if (s.stale && s.mode !== "human") out.push({ id: "stale", text: t("browser.nota_stale") });
  if (s.mode !== "human" && !s.dialog) out.push({ id: "readonly", text: t("browser.nota_sola_lettura") });
  out.push({ id: "headless", text: s.headless ? t("browser.nota_senza_finestra") : t("browser.nota_con_finestra") });
  return out;
}

// ---------------------------------------------------------------- what the person can do

export interface Controls {
  canPause: boolean;
  canResume: boolean;
  canTake: boolean;
  canGiveBack: boolean;
  /** The viewer forwards clicks and keys and the typing controls work: the person drives, nothing of the agent is in flight, no window waits. */
  canType: boolean;
  /** A window of the site can be answered: the person drives and no action of the agent is still running (the engine refuses the answer otherwise). */
  canAnswerDialog: boolean;
}

/** The controls each state allows. Pausing is the agent's: once the person drives, the agent already waits. */
export function controlsFor(s: BrowserState): Controls {
  const none: Controls = { canPause: false, canResume: false, canTake: false, canGiveBack: false, canType: false, canAnswerDialog: false };
  if (!s.running) return none;
  const human = s.mode === "human";
  return {
    canPause: s.mode === "agent",
    canResume: s.mode === "paused",
    canTake: !human,
    canGiveBack: human,
    canType: human && !s.agent_busy && s.dialog === null,
    canAnswerDialog: human && !s.agent_busy && s.dialog !== null,
  };
}

// ---------------------------------------------------------------- the window of the site

/** What a window of the site needs: a field for a `prompt`, a way out for everything but an `alert`. */
export function dialogShape(d: Pick<BrowserDialog, "kind">): { field: boolean; cancel: boolean; title: string } {
  switch (d.kind) {
    case "prompt": return { field: true, cancel: true, title: t("browser.finestra_prompt") };
    case "alert": return { field: false, cancel: false, title: t("browser.finestra_avviso") };
    case "confirm": return { field: false, cancel: true, title: t("browser.finestra_conferma") };
    default: return { field: false, cancel: true, title: t("browser.finestra_altro") };
  }
}

/** The message of the site, cut when it is very long (it is the site's text). */
export const clipDialogMessage = (m: string): string => {
  const chars = Array.from(m);
  return chars.length > MAX_DIALOG_CHARS ? `${chars.slice(0, MAX_DIALOG_CHARS).join("")}…` : m;
};

// ---------------------------------------------------------------- keys, clicks, text

/** The keys the engine accepts besides one printable character. Tab is not here: the viewer lets it leave (see `keyAction`). */
export const NAMED_KEYS = ["Enter", "Backspace", "Escape", "ArrowLeft", "ArrowUp", "ArrowRight", "ArrowDown", "Delete", "Home", "End", "PageUp", "PageDown"] as const;
/** Sent only by the buttons of the panel. */
export type ButtonKey = "Tab" | "Enter" | "Backspace";

export const keyInput = (key: string, shift = false): BrowserInput => ({ type: "key", key, shift });

/** The part of a keyboard event the mapping reads (a real `KeyboardEvent` fits). */
export interface KeyLike {
  key: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  isComposing?: boolean;
  getModifierState?: (key: string) => boolean;
}

export type KeyAction = { action: "send"; input: BrowserInput } | { action: "pass" };

/**
 * What a key pressed in the viewer does: it goes to the page (`send`) or it stays with the browser (`pass`). Tab and Shift+Tab pass, so
 * that the viewer is never a keyboard trap; shortcuts (Ctrl, Cmd) pass too, the engine takes no other shortcut. AltGr, and Alt on
 * a Mac keyboard, make characters (`@`, `#`, `[`): they are sent as the character.
 */
export function keyAction(e: KeyLike): KeyAction {
  const pass: KeyAction = { action: "pass" };
  if (e.isComposing || e.key === "Process" || e.key === "Dead" || e.key === "Unidentified") return pass;
  if (e.key === "Tab") return pass;
  const altGraph = e.getModifierState?.("AltGraph") === true;
  const shortcut = e.metaKey || (e.ctrlKey && !altGraph);
  if (shortcut) return pass;
  if ((NAMED_KEYS as readonly string[]).includes(e.key)) {
    return e.altKey && !altGraph ? pass : { action: "send", input: keyInput(e.key, e.shiftKey) };
  }
  if (Array.from(e.key).length === 1) return { action: "send", input: keyInput(e.key, e.shiftKey) };
  return pass;
}

/** The box of the frame on the screen. */
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

const clamp01 = (n: number): number => Math.min(1, Math.max(0, n));
const round4 = (n: number): number => Math.round(n * 10_000) / 10_000;

/** A point of the screen as fractions (0 to 1) of the frame; `null` when the frame has no size to speak of. */
export function fractionIn(box: Box, clientX: number, clientY: number): { x: number; y: number } | null {
  if (!(box.width > 0) || !(box.height > 0)) return null;
  return { x: round4(clamp01((clientX - box.left) / box.width)), y: round4(clamp01((clientY - box.top) / box.height)) };
}

/** A click of the person on the frame: a double click is two clicks, the second with `count: 2`. */
export function clickInput(box: Box, e: { clientX: number; clientY: number; detail?: number }): BrowserInput | null {
  const p = fractionIn(box, e.clientX, e.clientY);
  if (!p) return null;
  return { type: "click", x: p.x, y: p.y, button: "left", count: Math.min(3, Math.max(1, Math.round(e.detail ?? 1))) };
}

const MAX_SCROLL = 1_500;

/** A wheel turn in the pixels of the page (`deltaMode`: 0 pixels, 1 lines, 2 pages), within what one request may ask. */
export function wheelPixels(deltaY: number, deltaMode: number, pageHeight: number): number {
  const px = deltaMode === 1 ? deltaY * 16 : deltaMode === 2 ? deltaY * Math.max(100, pageHeight) : deltaY;
  return Math.round(Math.max(-MAX_SCROLL, Math.min(MAX_SCROLL, px)));
}

export function scrollInput(box: Box, clientX: number, clientY: number, dy: number): BrowserInput | null {
  const p = fractionIn(box, clientX, clientY);
  return p && dy !== 0 ? { type: "scroll", x: p.x, y: p.y, dy } : null;
}

/** The text cut into pieces of at most `max` characters (code points, not UTF-16 units: a pasted emoji is one). */
export function splitText(value: string, max = MAX_TEXT_CHARS): string[] {
  const chars = Array.from(value.replace(/\r\n?/g, "\n"));
  const out: string[] = [];
  for (let i = 0; i < chars.length; i += max) out.push(chars.slice(i, i + max).join(""));
  return out;
}

// ---------------------------------------------------------------- how often to look

/**
 * The time to the next frame, `null` for none: the tab is hidden, no browser runs, or a window of the site waits (the engine answers 409 to
 * a frame then, so the last one stays and nothing asks until the window is gone). About two a second while the person drives (they see
 * what they type); slower while the agent works or waits, when little changes under their eyes.
 */
export function frameDelay(o: { running: boolean; mode: BrowserMode; agentBusy: boolean; hidden: boolean; dialog: boolean }): number | null {
  if (!o.running || o.hidden || o.dialog) return null;
  if (o.mode === "human" && !o.agentBusy) return 500;
  return o.mode === "agent" && o.agentBusy ? 1_500 : 1_000;
}
