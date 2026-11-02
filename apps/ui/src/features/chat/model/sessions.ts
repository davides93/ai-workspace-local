import { matchesQuery } from "../../../shared/lib/text";
import type { Session } from "../../../shared/api/types";

/** Longest title the backend accepts (`MAX_TITLE_CHARS`). */
export const MAX_TITLE = 200;

export type DayGroupId = "today" | "yesterday" | "week" | "older";

export const DAY_GROUP_LABEL: Record<DayGroupId, string> = {
  today: "Oggi",
  yesterday: "Ieri",
  week: "Questa settimana",
  older: "Prima",
};

const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

/**
 * Day bucket of a session (calendar days in local time): today, yesterday, the five days before
 * that ("Questa settimana"), then everything older.
 */
export function dayGroup(iso: string, now: Date = new Date()): DayGroupId {
  const t = new Date(iso);
  if (Number.isNaN(t.getTime())) return "older";
  const days = Math.round((startOfDay(now) - startOfDay(t)) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days <= 6) return "week";
  return "older";
}

export interface SessionGroup {
  id: DayGroupId;
  label: string;
  sessions: Session[];
}

/** Most recently updated first. */
export const byRecent = (a: Session, b: Session): number => b.updated_at.localeCompare(a.updated_at);

/** Sessions grouped by day, newest first; empty groups are left out. */
export function groupSessions(sessions: Session[], now: Date = new Date()): SessionGroup[] {
  const groups = new Map<DayGroupId, Session[]>();
  for (const s of [...sessions].sort(byRecent)) {
    const g = dayGroup(s.updated_at, now);
    groups.set(g, [...(groups.get(g) ?? []), s]);
  }
  return (["today", "yesterday", "week", "older"] as const)
    .filter((id) => groups.has(id))
    .map((id) => ({ id, label: DAY_GROUP_LABEL[id], sessions: groups.get(id)! }));
}

/** Sessions whose title or agent name contains every word of the query. */
export function filterSessions(sessions: Session[], query: string, agentName: (agentId: string) => string | undefined): Session[] {
  if (!query.trim()) return sessions;
  return sessions.filter((s) => matchesQuery(`${s.title} ${agentName(s.agent_id) ?? s.agent_id}`, query));
}

/** A new title: spaces collapsed, not empty, not longer than the backend allows. */
export function validateTitle(raw: string): { ok: true; title: string } | { ok: false; error: string } {
  const title = raw.split(/\s+/).filter(Boolean).join(" ");
  if (!title) return { ok: false, error: "Il titolo non può essere vuoto." };
  if ([...title].length > MAX_TITLE) return { ok: false, error: `Il titolo supera i ${MAX_TITLE} caratteri.` };
  return { ok: true, title };
}
