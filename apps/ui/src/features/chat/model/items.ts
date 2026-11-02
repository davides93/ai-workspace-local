import type { ApprovalRequest } from "../../../shared/api/types";
import type { SurfaceState } from "../a2ui";

/** One row of the conversation, built from stored messages (`history.ts`) or live AG-UI events (`reduce.ts`). */
export type Item =
  | { kind: "user"; id: string; text: string }
  | { kind: "text"; id: string; text: string; streaming: boolean }
  | { kind: "reasoning"; id: string; text: string; streaming: boolean }
  | { kind: "tool"; id: string; name: string; args: string; result?: string; status: "running" | "done" | "denied"; images?: { mime: string; data: string }[] }
  | { kind: "approval"; req: ApprovalRequest; resolved?: boolean }
  | { kind: "subagent"; id: string; name: string; result?: string }
  | { kind: "note"; id: string; text: string }
  | { kind: "surface"; id: string; surface: SurfaceState }
  | { kind: "action"; id: string; name: string; context: Record<string, unknown> }
  | { kind: "error"; message: string };

export type ToolItem = Extract<Item, { kind: "tool" }>;
export type ApprovalItem = Extract<Item, { kind: "approval" }>;

/** Stored user messages written by the agent loop's self-check (not typed by the user). */
export const SELF_CHECK = "aiw.self_check";

export const selfCheckText = (failed: string[]) =>
  `Controllo automatico: ${failed.length === 1 ? "un'azione era fallita" : "alcune azioni erano fallite"} (${failed.join(", ")}). L'agente le riprende prima di concludere.`;
