// What voice mode says: the reply of the turn that just ended, and the announcement of a confirmation. Pure logic.

import type { ApprovalRequest } from "../../../shared/api/types";
import type { Item } from "./items";

export type Convo = "off" | "listening" | "transcribing" | "thinking" | "speaking";

export const CONVO_LABEL: Record<Exclude<Convo, "off">, string> = {
  listening: "Ti ascolto…",
  transcribing: "Trascrivo…",
  thinking: "L'agente sta lavorando…",
  speaking: "Rispondo…",
};

/** The text of the agent's last turn (everything after the last user message), or why it did not finish. */
export function replyToSpeak(items: readonly Item[]): string {
  let lastUser = -1;
  items.forEach((it, i) => {
    if (it.kind === "user") lastUser = i;
  });
  const after = items.slice(lastUser + 1);
  const reply = after.filter((i): i is Extract<Item, { kind: "text" }> => i.kind === "text").map((i) => i.text.trim()).filter(Boolean).join("\n\n");
  if (reply) return reply;
  const failed = after.find((i): i is Extract<Item, { kind: "error" }> => i.kind === "error");
  return failed ? `Non sono riuscito a completare: ${failed.message}` : "";
}

/** The sentence read aloud when the agent waits for a confirmation. */
export const approvalAnnouncement = (req: Pick<ApprovalRequest, "reason">): string => `Serve la tua conferma: ${req.reason}`;
