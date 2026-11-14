import type { AgEvent, AiwImage, ApprovalRequest } from "../../../shared/api/types";
import { applyA2ui, type A2uiMessage } from "../a2ui";
import { ASK_TOOL, asQuestionOutcome, asSecretOutcome, parseQuestion, parseQuestionResult, parseSecret, parseSecretResult, QUESTION_EVENT, QUESTION_RESOLVED_EVENT, SECRET_EVENT, SECRET_RESOLVED_EVENT, SECRET_TOOL, toAnswer } from "../../ask/model";
import { ATTACHMENTS_EVENT, CHECK, checkText, FORMAT_CHECK, formatCheckText, HANDOFF, modelLabel, readsExternalContent, rolesNoteText, SELF_CHECK, selfCheckText, type Item, type ToolImage } from "./items";
import { resultStatus } from "./tools";

import { t, t as tr } from "../../../shared/i18n";
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
    // Nobody can answer a question or provide a secret of a run that is over.
    if ((it.kind === "question" || it.kind === "secret") && it.outcome === undefined) {
      changed = true;
      return { ...it, outcome: "cancelled" };
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
      return closeAsk(items, e.toolCallId, e.content) ?? update(items, "tool", e.toolCallId, (it) => ({ ...it, result: e.content, status: resultStatus(e.content), endedAt: e.timestamp }));
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
    return [...items, { kind: "note", id: `rt-${items.length}`, text: `${v.fallback.note}.` }, { kind: "route", id: `rr-${items.length}`, label: t("chat.runtime", { name }), lines: routeLines(v) }];
  }
  return [...items, { kind: "route", id: `rr-${items.length}`, label: t("chat.runtime_automatico", { name }), lines: routeLines(v) }];
}

function routeLines(v: RuntimeEvent): string[] {
  const lines = [v.reason ? tr("chat.perche", { reason: v.reason }) : "", v.model ? tr(v.context ? "chat.model_with_context" : "chat.model_line", { model: v.model, context: v.context ?? "" }) : ""];
  for (const a of v.alternatives ?? []) lines.push(tr("chat.alternativa", { name: a.name ?? a.runtime ?? "", reason: a.reason ?? "" }));
  for (const x of v.excluded ?? []) lines.push(tr("chat.escluso", { name: x.name ?? x.runtime ?? "", reason: x.reason ?? "" }));
  return lines.filter((l) => l !== "");
}

/**
 * The row of a question or a request for a secret takes the place of the card of the tool call that asked it (the event
 * carries no call id: the call is the first one of that tool, in this turn, still running: the ones before it already have
 * their row). Without such a call the row is added at the end.
 */
function openAsk(items: Item[], tool: string, row: (callId: string | undefined) => Item): Item[] {
  let turn = 0;
  for (let i = items.length - 1; i >= 0; i--) {
    if (items[i].kind === "user" || items[i].kind === "action") {
      turn = i + 1;
      break;
    }
  }
  for (let i = turn; i < items.length; i++) {
    const it = items[i];
    if (it.kind === "tool" && it.name === tool && it.status === "running") {
      const next = items.slice();
      next[i] = row(it.id);
      return next;
    }
  }
  return [...items, row(undefined)];
}

/**
 * The result of the `ask_user` or `aiw_secret_request` call closes its row when the event that closes it was missed.
 * `null`: no open row stands for this call (the normal tool card takes the result).
 */
function closeAsk(items: Item[], callId: string, content: string): Item[] | null {
  const i = items.findIndex((x) => (x.kind === "question" || x.kind === "secret") && x.callId === callId);
  if (i < 0) return null;
  const it = items[i];
  if ((it.kind !== "question" && it.kind !== "secret") || it.outcome !== undefined) return items;
  const next = items.slice();
  if (it.kind === "question") {
    const r = parseQuestionResult(content);
    next[i] = { ...it, outcome: r?.outcome ?? "cancelled", answer: r?.answer ?? null };
  } else {
    next[i] = { ...it, outcome: parseSecretResult(content) ?? "cancelled" };
  }
  return next;
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
      return [...items, { kind: "note", id: `ho-${items.length}`, text: tr("chat.la_conversazione_passa_a", { name: v.to_name ?? v.to ?? tr("chat.un_altro_agente") }) }];
    }
    case "aiw.budget_exceeded": {
      const reason = (value as { reason?: string }).reason;
      return [...items, { kind: "note", id: `bx-${items.length}`, text: tr(reason ? "chat.budget_exhausted_reason" : "chat.budget_del_run_esaurito_l", { reason: reason ?? "" }) }];
    }
    case ATTACHMENTS_EVENT: {
      const warning = (value as { warning?: unknown } | null)?.warning;
      return typeof warning === "string" && warning ? [...items, { kind: "warning", id: `aw-${items.length}`, text: warning }] : items;
    }
    case "aiw.format_error":
      return [...items, { kind: "note", id: `fe-${items.length}`, text: tr("chat.la_risposta_non_rispetta_lo") }];
    case "aiw.approval_request":
      return [...items, { kind: "approval", req: value as ApprovalRequest, ...(readsExternalContent(items) ? { afterExternal: true } : {}) }];
    case QUESTION_EVENT: {
      const q = parseQuestion(value);
      // The same question again (the events of a run replayed on re-attach): the row is already there.
      if (!q || items.some((x) => x.kind === "question" && x.id === q.id)) return items;
      return openAsk(items, ASK_TOOL, (callId) => ({ kind: "question", id: q.id, ...(callId ? { callId } : {}), q }));
    }
    case QUESTION_RESOLVED_EVENT: {
      const v = (value ?? {}) as { id?: string; outcome?: string; answer?: unknown };
      return items.map((x) =>
        x.kind === "question" && x.id === v.id && x.outcome === undefined ? { ...x, outcome: asQuestionOutcome(v.outcome) ?? "cancelled", answer: toAnswer(v.answer) } : x,
      );
    }
    case SECRET_EVENT: {
      const req = parseSecret(value);
      if (!req || items.some((x) => x.kind === "secret" && x.id === req.id)) return items;
      return openAsk(items, SECRET_TOOL, (callId) => ({ kind: "secret", id: req.id, ...(callId ? { callId } : {}), req }));
    }
    case SECRET_RESOLVED_EVENT: {
      const v = (value ?? {}) as { id?: string; outcome?: string };
      return items.map((x) =>
        x.kind === "secret" && x.id === v.id && x.outcome === undefined ? { ...x, outcome: asSecretOutcome(v.outcome) ?? "cancelled" } : x,
      );
    }
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
