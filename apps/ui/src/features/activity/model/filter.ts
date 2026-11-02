import { matchesQuery } from "../../../shared/lib/text";
import type { AuditEntry } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";

/** The backend returns only the latest entries of the audit trail (`audit(500)`). */
export const BACKEND_LIMIT = 500;

export const ALL = "all";

export interface AuditFilters {
  actor: string;
  decision: string;
  tool: string;
  q: string;
}

export const emptyFilters: AuditFilters = { actor: ALL, decision: ALL, tool: ALL, q: "" };

export const isFiltering = (f: AuditFilters): boolean => f.actor !== ALL || f.decision !== ALL || f.tool !== ALL || f.q.trim() !== "";

export const ACTOR_LABEL: Record<string, string> = { agent: "Agente", user: "Utente", workflow: "Workflow", system: "Sistema" };

/** Decisions of the audit trail: the policy answers (allow, ask, deny) and the user's (approved, rejected). */
export const DECISION_INFO: Record<string, { label: string; tone: PillTone }> = {
  allow: { label: "Consentito", tone: "ok" },
  approved: { label: "Approvato", tone: "ok" },
  ask: { label: "Conferma richiesta", tone: "warn" },
  deny: { label: "Negato", tone: "crit" },
  rejected: { label: "Rifiutato", tone: "crit" },
};

export const decisionInfo = (d: string): { label: string; tone: PillTone } => DECISION_INFO[d] ?? { label: d, tone: "neutral" };

/** Newest first (the backend already sends them so; this keeps the page right if it ever does not). */
export const newestFirst = (rows: AuditEntry[]): AuditEntry[] => [...rows].sort((a, b) => b.ts.localeCompare(a.ts));

export function filterAudit(rows: AuditEntry[], f: AuditFilters): AuditEntry[] {
  return rows.filter((a) => {
    if (f.actor !== ALL && a.actor !== f.actor) return false;
    if (f.decision !== ALL && (a.decision ?? "") !== (f.decision === "none" ? "" : f.decision)) return false;
    if (f.tool !== ALL && (a.tool ?? "") !== f.tool) return false;
    return matchesQuery(`${a.actor} ${a.action} ${a.tool ?? ""} ${a.decision ?? ""} ${a.detail} ${a.run_id ?? ""}`, f.q);
  });
}

/** Distinct decisions and tools present in the rows, for the filter menus. */
export function facets(rows: AuditEntry[]): { decisions: string[]; tools: string[]; hasUndecided: boolean } {
  const decisions = new Set<string>();
  const tools = new Set<string>();
  let hasUndecided = false;
  for (const a of rows) {
    if (a.decision) decisions.add(a.decision);
    else hasUndecided = true;
    if (a.tool) tools.add(a.tool);
  }
  const order = Object.keys(DECISION_INFO);
  return {
    decisions: [...decisions].sort((a, b) => (order.indexOf(a) + 1 || 99) - (order.indexOf(b) + 1 || 99) || a.localeCompare(b)),
    tools: [...tools].sort((a, b) => a.localeCompare(b)),
    hasUndecided,
  };
}

export interface Page<T> {
  rows: T[];
  /** 1-based, clamped to the pages that exist. */
  page: number;
  pages: number;
  /** 1-based position of the first and last row shown (0 when empty). */
  from: number;
  to: number;
  total: number;
}

export function paginate<T>(rows: T[], page: number, size: number): Page<T> {
  const pages = Math.max(1, Math.ceil(rows.length / size));
  const p = Math.min(Math.max(1, page), pages);
  const start = (p - 1) * size;
  const slice = rows.slice(start, start + size);
  return { rows: slice, page: p, pages, from: slice.length ? start + 1 : 0, to: start + slice.length, total: rows.length };
}

const pad = (n: number) => String(n).padStart(2, "0");

/** The file offered by "Esporta JSON": the filtered rows plus what was filtered and when. */
export function auditExport(rows: AuditEntry[], filters: AuditFilters, now: Date = new Date()): { filename: string; content: string } {
  const filename = `attivita-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}.json`;
  const content = JSON.stringify(
    {
      exported_at: now.toISOString(),
      source: `Audit trail di AI Workspace Local (le ultime ${BACKEND_LIMIT} voci)`,
      filters: isFiltering(filters) ? filters : null,
      count: rows.length,
      entries: rows,
    },
    null,
    2,
  );
  return { filename, content };
}
