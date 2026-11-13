import type { ApprovalRequest, Attachment } from "../../../shared/api/types";
import type { SurfaceState } from "../a2ui";

import { t } from "../../../shared/i18n";
/** `interrupted`: the run ended (stopped, crashed) before the tool answered. */
export type ToolStatus = "running" | "done" | "failed" | "denied" | "interrupted";

export interface ToolImage {
  mime: string;
  /** Base64, without the `data:` prefix. */
  data: string;
}

/**
 * An attachment of a user message. A message just sent still holds its bytes (`data`, base64):
 * the row shows them at once; a stored one is read from the backend when needed.
 */
export type UserAttachment = Attachment & { data?: string };

/** One row of the conversation, built from stored messages (`history.ts`) or live AG-UI events (`reduce.ts`). */
export type Item =
  | { kind: "user"; id: string; text: string; attachments?: UserAttachment[] }
  | { kind: "text"; id: string; text: string; streaming: boolean }
  | { kind: "reasoning"; id: string; text: string; streaming: boolean; startedAt?: number; endedAt?: number }
  | { kind: "tool"; id: string; name: string; args: string; result?: string; status: ToolStatus; images?: ToolImage[]; startedAt?: number; endedAt?: number }
  | { kind: "approval"; req: ApprovalRequest; resolved?: boolean }
  | { kind: "subagent"; id: string; name: string; /** Run of the sub-agent, when the backend named it: its events are its trace. */ runId?: string; result?: string; interrupted?: boolean }
  | { kind: "note"; id: string; text: string }
  /** A small warning about the attachments of the run (`aiw.attachments`): the model cannot see images. */
  | { kind: "warning"; id: string; text: string }
  /** Which model handled the calls that follow (runs with model roles only): a small label. */
  | { kind: "model"; id: string; label: string; title: string }
  /** The runtime the router chose for a run on `auto` (`aiw.runtime`): shown in Developer mode only. */
  | { kind: "route"; id: string; label: string; lines: string[] }
  | { kind: "surface"; id: string; surface: SurfaceState }
  | { kind: "action"; id: string; name: string; context: Record<string, unknown> }
  | { kind: "error"; id: string; message: string; code?: string; /** Rebuilt from the stored history (not announced as a live alert). */ stored?: boolean };

export type ToolItem = Extract<Item, { kind: "tool" }>;
export type ApprovalItem = Extract<Item, { kind: "approval" }>;

/** Summary line of a reasoning row: while it goes on, afterwards how long it took (known for live runs only). */
export function reasoningLabel(it: Extract<Item, { kind: "reasoning" }>): string {
  if (it.streaming) return t("chat.sta_ragionando");
  if (it.startedAt === undefined || it.endedAt === undefined) return t("chat.ragionamento");
  const seconds = Math.max(1, Math.round((it.endedAt - it.startedAt) / 1000));
  return t("chat.ha_ragionato_per", { seconds });
}

/** Stable React key of a row. */
export const itemKey = (it: Item, index: number): string => `${it.kind}-${it.kind === "approval" ? it.req.id : it.id}-${index}`;

/** Stored user messages written by the agent loop's self-check (not typed by the user). */
export const SELF_CHECK = "aiw.self_check";

/** Stored user messages written when a structured answer did not match the agent's schema. */
export const FORMAT_CHECK = "aiw.format_check";

/** `name` of the custom event about the attachments of a run (`warning`: the model cannot see images). */
export const ATTACHMENTS_EVENT = "aiw.attachments";

/** `name` of the stored messages written by a handoff to another agent (api.rs `HANDOFF_MESSAGE`). */
export const HANDOFF = "aiw.handoff";

/** The row for the user message that opens the next agent's run (`[Passaggio da X] summary`). */
export const handoffText = (content: string) => {
  const m = /^\[Passaggio da ([^\]]+)\]\s*([\s\S]*)$/.exec(content);
  return m ? t("chat.handoff_text", { name: m[1] ?? "", summary: m[2] ?? "" }) : content;
};

export const formatCheckText = (errors: string[]) =>
  t("chat.controllo_del_formato_la_risposta", { errors: errors.slice(0, 3).join("; ") + (errors.length > 3 ? "; …" : "") });

export const selfCheckText = (failed: string[]) =>
  t(failed.length === 1 ? "chat.self_check_one" : "chat.self_check_many", { list: failed.join(", ") });

/** `name` of the stored user messages that hand the final check's objection back to the model (crates/aiw-agent `CHECK_NAME`). */
export const CHECK = "aiw.check";

const ROLE_LABEL: Record<string, string> = { get plan() { return t("chat.pianifica"); }, get act() { return t("chat.esegue"); }, get check() { return t("chat.verifica"); }, get summarize() { return t("chat.riassume"); } };

/** The small label of a call in a run with model roles: «Pianifica · qwen3-8b». */
export function modelLabel(role: string | null | undefined, model: string | undefined): string {
  const r = role ? ROLE_LABEL[role] : undefined;
  return [r, model].filter(Boolean).join(" · ") || t("chat.modello");
}

/** The note for the result of the final check. */
export const checkText = (passed: boolean, note?: string | null) =>
  passed ? t("chat.verifica_finale_tutto_a_posto") : t("chat.final_check_failed", { note: note ?? t("chat.qualcosa_non_torna") });

/** The note for a run that uses one model although the agent asks for roles (a model does not fit, or the engine holds one at a time). */
export const rolesNoteText = (notes: string[]) => notes.join(" ");
