import type { ApprovalRequest } from "../../../shared/api/types";
import type { SurfaceState } from "../a2ui";

/** `interrupted`: the run ended (stopped, crashed) before the tool answered. */
export type ToolStatus = "running" | "done" | "failed" | "denied" | "interrupted";

export interface ToolImage {
  mime: string;
  /** Base64, without the `data:` prefix. */
  data: string;
}

/** One row of the conversation, built from stored messages (`history.ts`) or live AG-UI events (`reduce.ts`). */
export type Item =
  | { kind: "user"; id: string; text: string }
  | { kind: "text"; id: string; text: string; streaming: boolean }
  | { kind: "reasoning"; id: string; text: string; streaming: boolean }
  | { kind: "tool"; id: string; name: string; args: string; result?: string; status: ToolStatus; images?: ToolImage[]; startedAt?: number; endedAt?: number }
  | { kind: "approval"; req: ApprovalRequest; resolved?: boolean }
  | { kind: "subagent"; id: string; name: string; result?: string; interrupted?: boolean }
  | { kind: "note"; id: string; text: string }
  | { kind: "surface"; id: string; surface: SurfaceState }
  | { kind: "action"; id: string; name: string; context: Record<string, unknown> }
  | { kind: "error"; id: string; message: string; code?: string; /** Rebuilt from the stored history (not announced as a live alert). */ stored?: boolean };

export type ToolItem = Extract<Item, { kind: "tool" }>;
export type ApprovalItem = Extract<Item, { kind: "approval" }>;

/** Stable React key of a row. */
export const itemKey = (it: Item, index: number): string => `${it.kind}-${it.kind === "approval" ? it.req.id : it.id}-${index}`;

/** Stored user messages written by the agent loop's self-check (not typed by the user). */
export const SELF_CHECK = "aiw.self_check";

export const selfCheckText = (failed: string[]) =>
  `Controllo automatico: ${failed.length === 1 ? "un'azione era fallita" : "alcune azioni erano fallite"} (${failed.join(", ")}). L'agente le riprende prima di concludere.`;
