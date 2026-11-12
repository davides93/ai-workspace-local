import type { ProactiveStatus, Proposal, ProposalStatus } from "../../../shared/api/types";

/** Proposals waiting for the person: only real proposals, never the briefing (a note, not a task). */
export const waiting = (list: Proposal[]): Proposal[] => list.filter((p) => p.kind === "proposal" && p.status === "open");

/** The number on the badge of the "Proposte" section. */
export const badgeCount = (list: Proposal[] | undefined): number => (list ? waiting(list).length : 0);

/** The latest briefing that has something to say (open); a "nothing to report" note is already marked done and stays out of the way. */
export function latestBriefing(list: Proposal[]): Proposal | undefined {
  return list.filter((p) => p.kind === "briefing" && p.status === "open").sort((a, b) => b.created_at.localeCompare(a.created_at))[0];
}

/** What was decided or finished, newest decision first (the "Fatte" list). */
export function decided(list: Proposal[]): Proposal[] {
  return list
    .filter((p) => p.kind === "proposal" && p.status !== "open")
    .sort((a, b) => (b.decided_at ?? b.created_at).localeCompare(a.decided_at ?? a.created_at));
}

export const STATUS_LABEL: Record<ProposalStatus, string> = {
  open: "In attesa",
  approved: "In corso",
  done: "Fatta",
  dismissed: "Scartata",
  expired: "Scaduta",
};

export const STATUS_TONE: Record<ProposalStatus, "warn" | "info" | "ok" | "neutral"> = {
  open: "warn",
  approved: "info",
  done: "ok",
  dismissed: "neutral",
  expired: "neutral",
};

/** One line saying whether the assistant is on, and when it looks next. */
export function statusLine(s: ProactiveStatus | undefined, now: number = Date.now()): string {
  if (!s) return "";
  if (s.level === "off") return "L'assistente proattivo è spento: lo accendi dalle impostazioni.";
  if (s.stopped_by_kill_switch) return "Fermato dal pulsante di emergenza: riparte quando lo riaccendi dalle impostazioni.";
  if (s.skipped_reason) return s.skipped_reason;
  if (s.next_wake) {
    const min = Math.max(1, Math.round((new Date(s.next_wake).getTime() - now) / 60_000));
    return min < 90 ? `Riguarda tra circa ${min} minuti.` : "Riguarda più tardi.";
  }
  return "";
}

/** Edited text to send with an approval: nothing when it is the proposed one (the server then uses its own). */
export const editedInstruction = (p: Proposal, text: string): string | undefined => {
  const t = text.trim();
  return t && t !== p.instruction.trim() ? t : undefined;
};
