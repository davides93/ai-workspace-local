import type { AgEvent, AiwImage, ApprovalRequest } from "../../../shared/api/types";
import { applyA2ui, type A2uiMessage } from "../a2ui";
import { ATTACHMENTS_EVENT, CHECK, checkText, FORMAT_CHECK, formatCheckText, HANDOFF, modelLabel, rolesNoteText, SELF_CHECK, selfCheckText, type Item, type ToolImage } from "./items";
import { resultStatus } from "./tools";

type Of<K extends Item["kind"]> = Extract<Item, { kind: K }>;

/** Index of the row of `kind` with this id, searching from the end (the live rows are the last ones). */
function indexOf(items: Item[], kind: Item["kind"], id: string): number {
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i];
    if (it.kind === kind && "id" in it && it.id === id) return i;
  }
  return -1;
}

function update<K extends "text" | "reasoning" | "tool" | "subagent">(items: Item[], kind: K, id: string, fn: (it: Of<K>) => Of<K>): Item[] {
  const i = indexOf(items, kind, id);
  if (i < 0) return items;
  const next = items.slice();
  next[i] = fn(items[i] as Of<K>);
  return next;
}

/** A well-formed `aiw.image` custom event: `{ toolCallId, mime, data }` with `data` as base64 (no `data:` prefix). */
export function parseImageEvent(value: unknown): { toolCallId: string; image: ToolImage } | null {
  if (!value || typeof value !== "object") return null;
  const v = value as Partial<AiwImage>;
  if (typeof v.toolCallId !== "string" || typeof v.mime !== "string" || typeof v.data !== "string") return null;
  if (!/^image\/[a-z0-9.+-]+$/i.test(v.mime)) return null;
  // Tolerate a full data URL: keep only the payload.
  const data = v.data.startsWith("data:") ? v.data.slice(v.data.indexOf(",") + 1) : v.data;
  if (!data || !/^[A-Za-z0-9+/_=-]+$/.test(data.slice(0, 256))) return null;
  return { toolCallId: v.toolCallId, image: { mime: v.mime, data } };
}

/**
 * The run is over (finished, failed or stopped): nothing is streaming, running or waiting any more.
 * Returns the same array when there is nothing to settle.
 */
export function settle(items: Item[]): Item[] {
  let changed = false;
  const next = items.map((it): Item => {
    if ((it.kind === "text" || it.kind === "reasoning") && it.streaming) {
      changed = true;
      return { ...it, streaming: false };
    }
    if (it.kind === "tool" && it.status === "running") {
      changed = true;
      return { ...it, status: "interrupted" };
    }
    if (it.kind === "subagent" && it.result === undefined && !it.interrupted) {
      changed = true;
      return { ...it, interrupted: true };
    }
    if (it.kind === "approval" && it.resolved === undefined) {
      changed = true;
      return { ...it, resolved: false };
    }
    return it;
  });
  return changed ? next : items;
}

/** Folds one AG-UI event into the conversation rows (pure: returns a new array). */
export function reduce(items: Item[], e: AgEvent): Item[] {
  switch (e.type) {
    case "TEXT_MESSAGE_START":
      return [...items, { kind: "text", id: e.messageId, text: "", streaming: true }];
    case "TEXT_MESSAGE_CONTENT":
      if (indexOf(items, "text", e.messageId) < 0) return [...items, { kind: "text", id: e.messageId, text: e.delta, streaming: true }];
      return update(items, "text", e.messageId, (it) => ({ ...it, text: it.text + e.delta }));
    case "TEXT_MESSAGE_END":
      return update(items, "text", e.messageId, (it) => ({ ...it, streaming: false }));
    case "REASONING_START":
      return [...items, { kind: "reasoning", id: e.messageId, text: "", streaming: true, startedAt: e.timestamp }];
    case "REASONING_MESSAGE_CONTENT":
      return update(items, "reasoning", e.messageId, (it) => ({ ...it, text: it.text + e.delta }));
    case "REASONING_END":
      return update(items, "reasoning", e.messageId, (it) => ({ ...it, streaming: false, endedAt: e.timestamp }));
    case "TOOL_CALL_START":
      return [...items, { kind: "tool", id: e.toolCallId, name: e.toolCallName, args: "", status: "running", startedAt: e.timestamp }];
    case "TOOL_CALL_ARGS":
      return update(items, "tool", e.toolCallId, (it) => ({ ...it, args: it.args + e.delta }));
    case "TOOL_CALL_RESULT":
      return update(items, "tool", e.toolCallId, (it) => ({ ...it, result: e.content, status: resultStatus(e.content), endedAt: e.timestamp }));
    case "SUBAGENT_STARTED":
      return [...items, { kind: "subagent", id: e.subagentId, name: e.name, ...(e.runId ? { runId: e.runId } : {}) }];
    case "SUBAGENT_FINISHED":
      return update(items, "subagent", e.subagentId, (it) => ({ ...it, result: e.result ?? "completato" }));
    case "CUSTOM":
      return reduceCustom(items, e.name, e.value);
    case "RUN_FINISHED":
      return settle(items);
    case "RUN_ERROR":
      return [...settle(items), { kind: "error", id: `err-${items.length}`, message: e.message, code: e.code }];
    default:
      return items;
  }
}

interface RuntimeEvent {
  runtime?: string;
  name?: string;
  model?: string;
  context?: number;
  reason?: string;
  alternatives?: { name?: string; runtime?: string; reason?: string }[];
  excluded?: { name?: string; runtime?: string; reason?: string }[];
  fallback?: { from?: string; note?: string } | null;
}

/**
 * The runtime router (`aiw.runtime`): the first event says what it chose and why (a Developer-mode row); a
 * later one says that it moved on to another runtime, and that is a note for everybody.
 */
export function reduceRuntime(items: Item[], value: unknown): Item[] {
  const v = (value ?? {}) as RuntimeEvent;
  const name = v.name ?? v.runtime ?? "runtime";
  if (v.fallback?.note) {
    return [...items, { kind: "note", id: `rt-${items.length}`, text: `${v.fallback.note}.` }, { kind: "route", id: `rr-${items.length}`, label: `Runtime: ${name}`, lines: routeLines(v) }];
  }
  return [...items, { kind: "route", id: `rr-${items.length}`, label: `Runtime automatico: ${name}`, lines: routeLines(v) }];
}

function routeLines(v: RuntimeEvent): string[] {
  const lines = [v.reason ? `Perché: ${v.reason}` : "", v.model ? `Modello: ${v.model}${v.context ? ` · contesto ${v.context}` : ""}` : ""];
  for (const a of v.alternatives ?? []) lines.push(`Alternativa · ${a.name ?? a.runtime}: ${a.reason ?? ""}`);
  for (const x of v.excluded ?? []) lines.push(`Escluso · ${x.name ?? x.runtime}: ${x.reason ?? ""}`);
  return lines.filter((l) => l !== "");
}

function reduceCustom(items: Item[], name: string, value: unknown): Item[] {
  switch (name) {
    case "a2ui": {
      const v = value as { surfaceId: string; messages: A2uiMessage[] };
      const i = items.findIndex((x) => x.kind === "surface" && x.id === v.surfaceId);
      if (i >= 0) {
        const it = items[i] as Of<"surface">;
        const surface = applyA2ui(it.surface, v.messages);
        if (!surface) return items;
        const next = items.slice();
        next[i] = { ...it, surface };
        return next;
      }
      const surface = applyA2ui(null, v.messages);
      return surface ? [...items, { kind: "surface", id: v.surfaceId, surface }] : items;
    }
    case SELF_CHECK:
      return [...items, { kind: "note", id: `sc-${items.length}`, text: selfCheckText((value as { failed?: string[] }).failed ?? []) }];
    case "aiw.model": {
      const v = value as { role?: string | null; model?: string; runtime?: string };
      const label = modelLabel(v.role, v.model);
      // Only when it changes within this turn: a run of calls by the same model gets one label.
      const turnStart = items.map((x) => x.kind).lastIndexOf("user") + 1;
      const last = items.slice(turnStart).filter((x) => x.kind === "model").pop();
      if (last && last.kind === "model" && last.label === label) return items;
      return [...items, { kind: "model", id: `md-${items.length}`, label, title: [v.runtime, v.model].filter(Boolean).join(" · ") }];
    }
    case "aiw.runtime":
      return reduceRuntime(items, value);
    case "aiw.roles": {
      const notes = (value as { notes?: string[] }).notes ?? [];
      return notes.length > 0 ? [...items, { kind: "note", id: `rl-${items.length}`, text: rolesNoteText(notes) }] : items;
    }
    case CHECK: {
      const v = value as { passed?: boolean; note?: string | null };
      return [...items, { kind: "note", id: `ck-${items.length}`, text: checkText(v.passed !== false, v.note) }];
    }
    case FORMAT_CHECK:
      return [...items, { kind: "note", id: `fc-${items.length}`, text: formatCheckText((value as { errors?: string[] }).errors ?? []) }];
    case HANDOFF: {
      const v = value as { to?: string; to_name?: string };
      return [...items, { kind: "note", id: `ho-${items.length}`, text: `La conversazione passa a ${v.to_name ?? v.to ?? "un altro agente"}.` }];
    }
    case "aiw.budget_exceeded": {
      const reason = (value as { reason?: string }).reason;
      return [...items, { kind: "note", id: `bx-${items.length}`, text: `Budget del run esaurito${reason ? `: ${reason}` : ""}. L'agente si è fermato.` }];
    }
    case ATTACHMENTS_EVENT: {
      const warning = (value as { warning?: unknown } | null)?.warning;
      return typeof warning === "string" && warning ? [...items, { kind: "warning", id: `aw-${items.length}`, text: warning }] : items;
    }
    case "aiw.format_error":
      return [...items, { kind: "note", id: `fe-${items.length}`, text: "La risposta non rispetta lo schema richiesto dall'agente: è mostrata così com'è." }];
    case "aiw.approval_request":
      return [...items, { kind: "approval", req: value as ApprovalRequest }];
    case "aiw.approval_resolved": {
      const v = value as { id: string; allowed: boolean };
      return items.map((x) => (x.kind === "approval" && x.req.id === v.id ? { ...x, resolved: v.allowed } : x));
    }
    case "aiw.image": {
      // Screenshot of a browser or computer-use tool: shown inside the card of that tool call.
      const ev = parseImageEvent(value);
      if (!ev) return items;
      return update(items, "tool", ev.toolCallId, (it) =>
        it.images?.some((x) => x.data === ev.image.data) ? it : { ...it, images: [...(it.images ?? []), ev.image] },
      );
    }
    default:
      return items;
  }
}
