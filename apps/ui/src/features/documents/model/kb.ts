// Pure logic of the page "Documenti": what to send, in which batches, and the sentences that describe a job.
// Limits mirror the engine (crates/aiw-engine/src/kb.rs and attachments.rs).

import { MAX_ATTACHMENTS, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS_TOTAL_BYTES, humanSize } from "../../../shared/lib/attachments";
import type { KbCollection, KbDocument, KbJob, KbStatus } from "../../../shared/api/types";

/** Shown while no embedding model is running: search still works, by keywords. */
export const KEYWORDS_ONLY_BANNER = "La ricerca funziona per parole chiave. Per trovare anche per significato scarica un modello di embedding in Modelli.";

export interface Batches<T extends { name: string; size: number }> {
  /** Groups to send one call each: at most 8 files and 25 MB in a group. */
  batches: T[][];
  /** One Italian sentence per file that cannot be sent. */
  errors: string[];
}

/** Splits the chosen files into groups the server accepts; empty and too big files are left out with the reason. */
export function splitBatches<T extends { name: string; size: number }>(files: T[]): Batches<T> {
  const batches: T[][] = [];
  const errors: string[] = [];
  let cur: T[] = [];
  let total = 0;
  for (const f of files) {
    if (f.size === 0) {
      errors.push(`«${f.name}» è vuoto: non c'è nulla da indicizzare.`);
      continue;
    }
    if (f.size > MAX_ATTACHMENT_BYTES) {
      errors.push(`«${f.name}» è troppo grande (${humanSize(f.size)}): ogni file può pesare al massimo ${humanSize(MAX_ATTACHMENT_BYTES)}.`);
      continue;
    }
    if (cur.length >= MAX_ATTACHMENTS || total + f.size > MAX_ATTACHMENTS_TOTAL_BYTES) {
      batches.push(cur);
      cur = [];
      total = 0;
    }
    cur.push(f);
    total += f.size;
  }
  if (cur.length > 0) batches.push(cur);
  return { batches, errors };
}

export const isRunning = (j: KbJob) => j.state === "running";

/** Running jobs of a collection (all of them when no collection is given). */
export function runningJobs(status: KbStatus | undefined, collectionId?: string): KbJob[] {
  return (status?.jobs ?? []).filter((j) => isRunning(j) && (collectionId === undefined || j.collection_id === collectionId));
}

/** 0 to 100. */
export function jobPercent(j: KbJob): number {
  if (j.state === "done") return 100;
  return j.total <= 0 ? 0 : Math.min(100, Math.round((j.done / j.total) * 100));
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** One sentence with the outcome of a finished (or cancelled) job. */
export function jobSummary(j: KbJob): string {
  const parts: string[] = [];
  if (j.added > 0) parts.push(`${plural(j.added, "documento aggiunto", "documenti aggiunti")}`);
  if (j.replaced > 0) parts.push(`${plural(j.replaced, "aggiornato", "aggiornati")}`);
  if (j.unchanged > 0) parts.push(`${plural(j.unchanged, "già presente", "già presenti")}`);
  if (j.failed > 0) parts.push(`${plural(j.failed, "non riuscito", "non riusciti")}`);
  if (j.skipped_total > 0) parts.push(`${plural(j.skipped_total, "file saltato", "file saltati")}`);
  const body = parts.length > 0 ? parts.join(", ") : "nessuna novità";
  return j.state === "cancelled" ? `Interrotto: ${body}.` : `${body.charAt(0).toUpperCase()}${body.slice(1)}.`;
}

/** What the job is doing now, for the progress line. */
export function jobProgressText(j: KbJob): string {
  const base = `${j.done} di ${j.total}`;
  return j.current ? `${base} · ${j.current}` : base;
}

export function documentStatusLabel(d: KbDocument): string {
  if (d.status === "indexing") return "In lavorazione";
  if (d.status === "failed") return "Non riuscito";
  return "Pronto";
}

/** How much of the collection can be searched by meaning, as a sentence (empty when nothing is indexed). */
export function coverageText(c: KbCollection): string {
  if (c.chunks === 0) return "";
  if (c.embedded_chunks === 0) return "Ricerca per parole chiave";
  if (c.embedded_chunks >= c.chunks) return "Ricerca anche per significato";
  return `Ricerca per significato pronta su ${c.embedded_chunks} passaggi su ${c.chunks}`;
}

export const collectionCounts = (c: KbCollection) => `${plural(c.documents, "documento", "documenti")}`;

/** The id of the collection to show: the wanted one if it still exists, else the first. */
export function pickCollection(list: readonly KbCollection[] | undefined, wanted: string | null): string | null {
  if (!list || list.length === 0) return null;
  return list.some((c) => c.id === wanted) ? wanted : list[0].id;
}
