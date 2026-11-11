import { MAX_MEMORY_CHARS } from "../types";
import type { Memory, MemoryCreate, MemoryKind, MemoryPatch } from "../types";

// Demo data and rules of the memory API, as the engine applies them (crates/aiw-engine/src/memory.rs).

const iso = (minAgo: number) => new Date(Date.now() - minAgo * 60_000).toISOString();

export const MEMORY_KINDS: readonly MemoryKind[] = ["about_you", "preference", "how_to_work", "fact", "reminder"];

export const memories: Memory[] = [
  { id: "mem_demo_name", text: "Si chiama Giulia e vive a Torino.", kind: "about_you", scope: "all", pinned: true, created_at: iso(9000), updated_at: iso(9000), last_used_at: iso(30), status: "active" },
  { id: "mem_demo_short", text: "Preferisce risposte brevi, con un elenco puntato quando ci sono più cose.", kind: "preference", scope: "all", pinned: true, created_at: iso(8000), updated_at: iso(8000), last_used_at: iso(30), status: "active" },
  { id: "mem_demo_coffee", text: "Beve il caffè senza zucchero.", kind: "preference", scope: "all", pinned: false, created_at: iso(5000), updated_at: iso(5000), status: "active" },
  { id: "mem_demo_work", text: "Quando riordina una cartella vuole prima vedere l'elenco dei file da spostare.", kind: "how_to_work", scope: "all", pinned: false, created_at: iso(3000), updated_at: iso(3000), status: "active" },
  { id: "mem_demo_fact", text: "Lavora tre giorni a settimana da casa: lunedì, mercoledì e venerdì.", kind: "fact", scope: "all", pinned: false, created_at: iso(2000), updated_at: iso(2000), status: "active" },
  { id: "mem_demo_reminder", text: "Rinnovare l'abbonamento del treno.", kind: "reminder", scope: "all", pinned: false, created_at: iso(1000), updated_at: iso(1000), due_at: new Date(Date.now() + 5 * 86_400_000).toISOString(), status: "active" },
  { id: "mem_demo_proposed", text: "Forse preferisce viaggiare in treno invece che in aereo.", kind: "preference", scope: "all", pinned: false, created_at: iso(60), updated_at: iso(60), status: "proposed" },
];

const STOP = new Set(["il", "lo", "la", "le", "gli", "un", "uno", "una", "di", "da", "in", "con", "su", "per", "tra", "fra", "ed", "ma", "che", "chi", "non", "mi", "ti", "ci", "si", "del", "dei", "al", "ai", "nel", "sul", "the", "an", "of", "to", "and", "is", "my", "are", "sono", "cosa", "come", "ho", "hai", "ha", "mio", "mia", "tuo", "tua", "me", "te", "se", "no", "poi"]);

/** Lowercase words without accents and stop words (the engine's `tokenize`). */
export function tokenize(text: string): string[] {
  return text
    .toLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .split(/[^\p{L}\p{N}]+/u)
    .filter((w) => w.length >= 2 && !STOP.has(w));
}

/** Keyword ranking: words of the query that start a word of the memory, best first (the engine's `keyword_rank`). */
export function rank(query: string, list: Memory[]): Memory[] {
  const q = tokenize(query);
  if (q.length === 0) return [];
  return list
    .map((m) => {
      const words = tokenize(m.text);
      const hits = q.filter((w) => words.some((t) => t.startsWith(w) || (t.length >= 4 && w.startsWith(t)))).length;
      return { m, hits };
    })
    .filter((x) => x.hits > 0)
    .sort((a, b) => b.hits - a.hits || b.m.updated_at.localeCompare(a.m.updated_at) || a.m.id.localeCompare(b.m.id))
    .map((x, i) => ({ ...x.m, score: 1 / (60 + i + 1) }));
}

/** The checks the engine makes on a text; the first problem is the message. */
export function textProblem(text: string): string | null {
  const t = text.trim();
  if (!t) return "il testo del ricordo non può essere vuoto";
  if ([...t].length > MAX_MEMORY_CHARS) return `il ricordo è troppo lungo (massimo ${MAX_MEMORY_CHARS} caratteri): dividilo in più ricordi`;
  return null;
}

export function kindProblem(kind: string | undefined): string | null {
  return kind === undefined || (MEMORY_KINDS as readonly string[]).includes(kind) ? null : `tipo di ricordo non valido «${kind}»: usa ${MEMORY_KINDS.join(", ")}`;
}

const clean = (text: string) => text.trim().split(/\s+/).join(" ");
const sameWords = (a: string, b: string) => tokenize(a).join(" ") === tokenize(b).join(" ");

/** Pinned first, then the most recently changed (the engine's listing order). */
export function ordered(list: Memory[]): Memory[] {
  return [...list].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.updated_at.localeCompare(a.updated_at) || a.id.localeCompare(b.id));
}

/** What a person adds by hand: active at once; the same words are the same memory (a forgotten one comes back). */
export function build(body: MemoryCreate, id: string): { memory: Memory; created: boolean } {
  const problem = textProblem(body.text) ?? kindProblem(body.kind);
  if (problem) throw new Error(problem);
  const text = clean(body.text);
  const existing = memories.find((m) => sameWords(m.text, text));
  if (existing) {
    if (existing.status === "forgotten" || existing.status === "proposed") Object.assign(existing, { status: "active", updated_at: new Date().toISOString() });
    return { memory: existing, created: false };
  }
  const now = new Date().toISOString();
  return {
    memory: { id, text, kind: body.kind ?? "fact", scope: body.scope?.trim() || "all", pinned: body.pinned ?? false, created_at: now, updated_at: now, ...(body.due_at ? { due_at: body.due_at } : {}), status: "active" },
    created: true,
  };
}

export function patch(m: Memory, p: MemoryPatch): void {
  if (p.text !== undefined) {
    const problem = textProblem(p.text);
    if (problem) throw new Error(problem);
    m.text = clean(p.text);
  }
  if (p.kind !== undefined) {
    const problem = kindProblem(p.kind);
    if (problem) throw new Error(problem);
    m.kind = p.kind;
  }
  if (p.scope !== undefined) m.scope = p.scope.trim() || "all";
  if (p.pinned !== undefined) m.pinned = p.pinned;
  if (p.due_at !== undefined) {
    if (p.due_at === "") delete m.due_at;
    else m.due_at = p.due_at;
  }
  m.updated_at = new Date().toISOString();
}
