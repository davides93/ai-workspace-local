import { RUN_ERROR_MESSAGE, type ChatMessage } from "../../../shared/api/types";
import { A2UI_ACTION_NAME, applyA2ui, describeAction, renderUiToMessages } from "../a2ui";
import { ASK_TOOL, parseQuestionArgs, parseQuestionResult, parseSecretArgs, parseSecretResult, SECRET_TOOL } from "../../ask/model";
import { CHECK, checkText, FORMAT_CHECK, formatCheckText, HANDOFF, handoffText, SELF_CHECK, selfCheckText, type Item, type ToolImage, type ToolItem } from "./items";
import { runErrorCode } from "./runNotice";
import { resultStatus } from "./tools";

export interface HistoryOptions {
  /** Screenshots seen while the tool ran: the stored messages carry only text. */
  images?: (toolCallId: string) => ToolImage[] | undefined;
}

/**
 * Rebuilds the conversation rows from the stored messages of a session.
 *
 * The backend stores what a run produced even when it was stopped or failed, so a conversation can
 * end with a tool call that never got its answer: it is shown as interrupted, not as running. A run
 * that ended in error also leaves a `system` message named `aiw.run_error`: it becomes the same error
 * row the live `RUN_ERROR` event produces, so a reload shows what the user saw. Any other `system`
 * message is not part of the conversation and is skipped.
 */
export function historyToItems(msgs: ChatMessage[], opts: HistoryOptions = {}): Item[] {
  const items: Item[] = [];
  for (const m of msgs) {
    if (m.role === "user" && m.name === A2UI_ACTION_NAME) {
      const a = describeAction(m.content);
      items.push(a ? { kind: "action", id: m.id, ...a } : { kind: "user", id: m.id, text: m.content });
    } else if (m.role === "user" && m.name === SELF_CHECK) {
      const failed = [...m.content.matchAll(/^- ([\w.-]+):/gm)].map((x) => x[1]);
      items.push({ kind: "note", id: m.id, text: selfCheckText(failed) });
    } else if (m.role === "user" && m.name === CHECK) {
      const objection = /problema: ([^\n]+)/.exec(m.content)?.[1];
      items.push({ kind: "note", id: m.id, text: checkText(false, objection) });
    } else if (m.role === "user" && m.name === FORMAT_CHECK) {
      const errors = [...m.content.matchAll(/^- (.+)$/gm)].map((x) => x[1]);
      items.push({ kind: "note", id: m.id, text: formatCheckText(errors) });
    } else if (m.role === "user" && m.name === HANDOFF) {
      items.push({ kind: "note", id: m.id, text: handoffText(m.content) });
    } else if (m.role === "user") items.push({ kind: "user", id: m.id, text: m.content, ...(m.attachments?.length ? { attachments: m.attachments } : {}) });
    else if (m.role === "assistant") {
      if (m.reasoning) items.push({ kind: "reasoning", id: `${m.id}-r`, text: m.reasoning, streaming: false });
      if (m.content) items.push({ kind: "text", id: m.id, text: m.content, streaming: false });
      for (const tc of m.tool_calls ?? []) {
        const images = opts.images?.(tc.id);
        items.push({ kind: "tool", id: tc.id, name: tc.name, args: tc.arguments, status: "interrupted", ...(images?.length ? { images } : {}) });
      }
    } else if (m.role === "system") {
      if (m.name === RUN_ERROR_MESSAGE) items.push({ kind: "error", id: m.id, message: m.content, code: runErrorCode(m.content), stored: true });
      else if (m.name === HANDOFF) items.push({ kind: "note", id: m.id, text: m.content });
    } else if (m.role === "tool") {
      const idx = items.findIndex((i) => i.kind === "tool" && i.id === m.tool_call_id);
      const card = items[idx] as ToolItem | undefined;
      if (card) {
        card.result = m.content;
        card.status = resultStatus(m.content);
        // A successful render_ui call is shown as the interface it produced.
        if (card.name === "render_ui" && card.status === "done") replaceWithSurface(items, idx, card);
        // A question or a request for a secret is shown as the line it became: what was asked and how it ended.
        else if (card.name === ASK_TOOL) replaceWithQuestion(items, idx, card);
        else if (card.name === SECRET_TOOL) replaceWithSecret(items, idx, card);
      }
    }
  }
  // A question that never got its answer (the run was stopped while it waited) is a closed line, not a running tool.
  items.forEach((it, idx) => {
    if (it.kind === "tool" && it.result === undefined && it.name === ASK_TOOL) replaceWithQuestion(items, idx, it);
    else if (it.kind === "tool" && it.result === undefined && it.name === SECRET_TOOL) replaceWithSecret(items, idx, it);
  });
  return items;
}

/**
 * `ask_user` call → closed question row. The row is built from the arguments of the call and its result; a call that
 * failed, was never shown (`unsupported`), or whose arguments do not parse keeps its tool card.
 */
function replaceWithQuestion(items: Item[], idx: number, card: ToolItem): void {
  const q = parseQuestionArgs(card.id, card.args);
  if (!q) return;
  const r = card.result === undefined ? { outcome: "cancelled" as const, answer: null } : parseQuestionResult(card.result);
  if (!r || r.outcome === "unsupported") return;
  items[idx] = { kind: "question", id: card.id, callId: card.id, q, outcome: r.outcome, answer: r.answer };
}

/** `aiw_secret_request` call → closed request row (only the label, the reason and how it ended: the stored call has no destination and no value). */
function replaceWithSecret(items: Item[], idx: number, card: ToolItem): void {
  const req = parseSecretArgs(card.id, card.args);
  if (!req) return;
  const outcome = card.result === undefined ? ("cancelled" as const) : parseSecretResult(card.result);
  if (!outcome || outcome === "unsupported") return;
  items[idx] = { kind: "secret", id: card.id, callId: card.id, req, outcome };
}

function replaceWithSurface(items: Item[], idx: number, card: ToolItem): void {
  try {
    const msgs = renderUiToMessages(JSON.parse(card.args) as Record<string, unknown>);
    const sid = msgs[0].createSurface!.surfaceId;
    const prev = items.findIndex((i) => i.kind === "surface" && i.id === sid);
    if (prev >= 0) {
      const it = items[prev] as Extract<Item, { kind: "surface" }>;
      it.surface = applyA2ui(it.surface, msgs) ?? it.surface;
      items.splice(idx, 1);
    } else {
      const surface = applyA2ui(null, msgs);
      if (surface) items[idx] = { kind: "surface", id: sid, surface };
    }
  } catch {
    /* malformed arguments: keep the tool card */
  }
}
