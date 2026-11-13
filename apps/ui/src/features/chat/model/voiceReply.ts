// What voice mode says: the reply of the turn that just ended, and the announcement of a confirmation. Pure logic.

import type { ApprovalRequest } from "../../../shared/api/types";
import type { Item } from "./items";

import { t } from "../../../shared/i18n";
export type Convo = "off" | "listening" | "transcribing" | "thinking" | "speaking";

export const CONVO_LABEL: Record<Exclude<Convo, "off">, string> = {
  get listening() { return t("chat.ti_ascolto"); },
  get transcribing() { return t("chat.trascrivo"); },
  get thinking() { return t("chat.l_agente_sta_lavorando"); },
  get speaking() { return t("chat.rispondo"); },
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
  return failed ? t("chat.non_sono_riuscito_a_completare", { message: failed.message }) : "";
}

/** The sentence read aloud when the agent waits for a confirmation. */
export const approvalAnnouncement = (req: Pick<ApprovalRequest, "reason">): string => t("chat.approval_announcement", { reason: req.reason });
