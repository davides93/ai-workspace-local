import type { Memory, MemoryKind, MemoryPatch, Session } from "../../../shared/api/types";

// Pure logic of the "Insegna" page: how memories are grouped and named, how the cached lists follow an
// action before the answer arrives, the export file and which session is the teaching one.

/** The built-in agent of the page (crates/aiw-engine/src/defaults.rs). */
export const TEACHER_ID = "teacher";

/** Kinds in the order the panel shows them. */
export const KIND_ORDER: readonly MemoryKind[] = ["about_you", "preference", "how_to_work", "fact", "reminder"];

/** Group names, as the person reads them. */
export const KIND_LABEL: Record<MemoryKind, string> = {
  about_you: "Su di te",
  preference: "Preferenze",
  how_to_work: "Come lavorare",
  fact: "Fatti",
  reminder: "Promemoria",
};

/** Name of a kind; a kind this version does not know (a newer engine) is shown with the plain "Fatti". */
export const kindLabel = (kind: string): string => KIND_LABEL[kind as MemoryKind] ?? KIND_LABEL.fact;

/** Three sentences that show what the page is for; a tap sends one. */
export const EXAMPLES: readonly string[] = [
  "Mi chiamo Giulia e preferisco che mi dai del tu.",
  "Rispondi sempre in modo breve, con un elenco puntato.",
  "Ricordami di rinnovare l'abbonamento del treno entro venerdì.",
];

export interface MemoryGroup {
  kind: MemoryKind;
  label: string;
  items: Memory[];
}

/** Pinned first, then the most recently changed; the id keeps the order stable. */
export function byImportance(a: Memory, b: Memory): number {
  return Number(b.pinned) - Number(a.pinned) || b.updated_at.localeCompare(a.updated_at) || a.id.localeCompare(b.id);
}

export interface SplitMemories {
  /** What the assistant guessed and waits for the person's yes or no. */
  proposed: Memory[];
  /** What it knows, by kind; kinds with nothing are left out. */
  groups: MemoryGroup[];
  /** How many memories are in use (not counting proposals). */
  known: number;
}

/** Splits a list into the proposals and the known memories by kind; forgotten ones are not shown. */
export function splitMemories(list: readonly Memory[] | undefined): SplitMemories {
  const all = list ?? [];
  const proposed = all.filter((m) => m.status === "proposed").sort(byImportance);
  const active = all.filter((m) => m.status === "active");
  const groups = KIND_ORDER.map((kind) => ({
    kind,
    label: KIND_LABEL[kind],
    // A kind from a newer engine lands in "Fatti" rather than disappearing.
    items: active.filter((m) => (KIND_ORDER.includes(m.kind) ? m.kind === kind : kind === "fact")).sort(byImportance),
  })).filter((g) => g.items.length > 0);
  return { proposed, groups, known: active.length };
}

// ---------------------------------------------------------------- optimistic updates of the cached lists

/** The memory with this id changed by `patch`, as the engine would apply it. */
export function patched(list: readonly Memory[], id: string, patch: MemoryPatch): Memory[] {
  return list.map((m) => {
    if (m.id !== id) return m;
    const next: Memory = { ...m };
    if (patch.text !== undefined) next.text = patch.text.trim().split(/\s+/).join(" ");
    if (patch.kind !== undefined) next.kind = patch.kind;
    if (patch.scope !== undefined) next.scope = patch.scope.trim() || "all";
    if (patch.pinned !== undefined) next.pinned = patch.pinned;
    if (patch.due_at !== undefined) {
      if (patch.due_at === "") delete next.due_at;
      else next.due_at = patch.due_at;
    }
    return next;
  });
}

/** A forgotten memory leaves the lists that show active and proposed ones; `keep` is for the one that lists everything. */
export function forgotten(list: readonly Memory[], id: string, keep = false): Memory[] {
  return keep ? list.map((m) => (m.id === id ? { ...m, status: "forgotten" as const } : m)) : list.filter((m) => m.id !== id);
}

/** A memory brought back (or approved) is in use again. */
export function activated(list: readonly Memory[], memory: Memory): Memory[] {
  const rest = list.filter((m) => m.id !== memory.id);
  return [{ ...memory, status: "active" }, ...rest];
}

export const without = (list: readonly Memory[], id: string): Memory[] => list.filter((m) => m.id !== id);

/** The memory with this id in a list, if it is there. */
export const findMemory = (list: readonly Memory[] | undefined, id: string): Memory | undefined => list?.find((m) => m.id === id);

// ---------------------------------------------------------------- reminders

/** "scade tra 5 giorni", "scade oggi", "scaduto": only for a reminder with a date. */
export function dueLabel(due: string | undefined, now: number = Date.now()): string | undefined {
  if (!due) return undefined;
  const t = new Date(due).getTime();
  if (Number.isNaN(t)) return undefined;
  if (t < now) return "scaduto";
  // Calendar days in the person's own time zone: «oggi» is until midnight, not the next 24 hours.
  const midnight = (ms: number) => new Date(new Date(ms).setHours(0, 0, 0, 0)).getTime();
  const days = Math.round((midnight(t) - midnight(now)) / 86_400_000);
  if (days <= 0) return "scade oggi";
  if (days === 1) return "scade domani";
  return `scade tra ${days} giorni`;
}

// ---------------------------------------------------------------- export

/** `ricordi-2026-11-11.json` */
export const exportFileName = (now: Date = new Date()): string => `ricordi-${now.toISOString().slice(0, 10)}.json`;

/** What "Esporta" downloads: the memories in use and the proposals, without the technical fields. */
export function exportJson(list: readonly Memory[]): string {
  const rows = list
    .filter((m) => m.status !== "forgotten")
    .sort(byImportance)
    .map((m) => ({
      testo: m.text,
      tipo: kindLabel(m.kind),
      kind: m.kind,
      sempre_presente: m.pinned,
      stato: m.status === "proposed" ? "da confermare" : "attivo",
      ...(m.due_at ? { scadenza: m.due_at } : {}),
      creato: m.created_at,
      id: m.id,
    }));
  return JSON.stringify({ versione: 1, ricordi: rows }, null, 2);
}

// ---------------------------------------------------------------- the teaching session

/** The one persistent "Insegna" session: the most recently used session of the teacher agent. */
export function teachSession(sessions: readonly Session[] | undefined): Session | undefined {
  return (sessions ?? []).filter((s) => s.agent_id === TEACHER_ID).sort((a, b) => b.updated_at.localeCompare(a.updated_at) || a.id.localeCompare(b.id))[0];
}
