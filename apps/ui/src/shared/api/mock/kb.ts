import type { AttachmentUpload, KbCollection, KbDocument, KbHit, KbJob, KbSearchBody, KbSearchResult } from "../types";
import { ApiError } from "../http";
import { MAX_ATTACHMENTS, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS_TOTAL_BYTES } from "../../lib/attachments";
import { tokenize } from "./memories";

// Demo data and rules of the knowledge base, as the engine applies them (crates/aiw-engine/src/kb.rs).

const iso = (minAgo: number) => new Date(Date.now() - minAgo * 60_000).toISOString();
let counter = 0;
const id = (p: string) => `${p}_${Date.now().toString(36)}${(counter++).toString(36)}`;

const CHUNK_TARGET = 1200;
const MAX_NAME_CHARS = 80;
const MAX_DESCRIPTION_CHARS = 300;
const MAX_RESULTS = 10;
const TEXT_EXTENSIONS = new Set([
  "txt", "md", "markdown", "csv", "tsv", "json", "xml", "html", "htm", "css", "js", "mjs", "ts", "tsx", "jsx", "py", "rs", "go", "java", "c", "h", "cpp", "sh", "yaml", "yml", "toml", "ini", "log", "sql",
]);

interface Doc extends KbDocument {
  texts: string[];
}

const doc = (collection: string, name: string, source: string, size: number, minAgo: number, texts: string[]): Doc => ({
  id: id("kbd"),
  collection_id: collection,
  name,
  source,
  mime: name.endsWith(".pdf") ? "application/pdf" : "text/plain",
  size,
  sha256: sha(name + texts.join("")),
  status: "ready",
  chunks: texts.length,
  embedded_chunks: 0,
  created_at: iso(minAgo),
  texts,
});

/** Not a real SHA-256: a stable 64-hex fingerprint is enough for the demo's "same content" rule. */
function sha(text: string): string {
  let h1 = 0x811c9dc5;
  let h2 = 0x01000193;
  for (let i = 0; i < text.length; i++) {
    h1 = Math.imul(h1 ^ text.charCodeAt(i), 16777619) >>> 0;
    h2 = Math.imul(h2 + text.charCodeAt(i), 2246822519) >>> 0;
  }
  const part = (n: number) => n.toString(16).padStart(8, "0");
  return (part(h1) + part(h2)).repeat(4);
}

const collections: Array<{ id: string; name: string; description: string; created_at: string }> = [
  { id: "kbk_demo_casa", name: "Casa", description: "Contratti, bollette e manuali degli elettrodomestici.", created_at: iso(4000) },
];

const docs: Doc[] = [
  doc("kbk_demo_casa", "contratto-affitto.txt", "caricato", 2300, 3900, [
    "Contratto di locazione ad uso abitativo. Il canone mensile è di 750 euro, da pagare entro il giorno 5 di ogni mese con bonifico bancario.",
    "La durata del contratto è di quattro anni, rinnovabili per altri quattro. Il conduttore può recedere con un preavviso di sei mesi.",
  ]),
  doc("kbk_demo_casa", "manuale-lavatrice.pdf", "caricato", 184_000, 3800, [
    "Lavatrice: per il programma Eco 40-60 non superare i 7 kg di bucato. Pulisci il filtro della pompa ogni due mesi.",
    "Se la lavatrice non scarica l'acqua, controlla il tubo di scarico e il filtro. Il codice errore E21 indica uno scarico bloccato.",
  ]),
  doc("kbk_demo_casa", "bolletta-luce.txt", "caricato", 1400, 3700, [
    "Bolletta della luce: consumo di 212 kWh nel bimestre, scadenza il 28 del mese. Il contratto è a prezzo fisso fino a giugno.",
  ]),
];

export const jobs: KbJob[] = [];
let embeddingModel: string | undefined;
/** The demo has no embedding model unless a test asks for one (the banner depends on it). */
export function setEmbeddingModel(name: string | undefined) {
  embeddingModel = name;
}

const clone = <T>(v: T): T => structuredClone(v);

function counts(colId: string): Pick<KbCollection, "documents" | "chunks" | "embedded_chunks"> {
  const mine = docs.filter((d) => d.collection_id === colId);
  const ready = mine.filter((d) => d.status === "ready");
  return {
    documents: mine.length,
    chunks: ready.reduce((n, d) => n + d.chunks, 0),
    embedded_chunks: ready.reduce((n, d) => n + d.embedded_chunks, 0),
  };
}

const view = (c: (typeof collections)[number]): KbCollection => ({ ...c, ...counts(c.id) });
const docView = ({ texts: _texts, ...d }: Doc): KbDocument => d;

function cleanName(name: string): string {
  const n = name.split(/\s+/).filter(Boolean).join(" ");
  if (!n) throw new ApiError(400, "Dai un nome alla raccolta.");
  if ([...n].length > MAX_NAME_CHARS) throw new ApiError(400, `Il nome della raccolta può avere al massimo ${MAX_NAME_CHARS} caratteri.`);
  return n;
}

function cleanDescription(d: string): string {
  const t = d.trim();
  if ([...t].length > MAX_DESCRIPTION_CHARS) throw new ApiError(400, `La descrizione può avere al massimo ${MAX_DESCRIPTION_CHARS} caratteri.`);
  return t;
}

const byKey = (key: string) => {
  const k = key.trim();
  return collections.find((c) => c.id === k) ?? collections.find((c) => c.name.toLowerCase() === k.toLowerCase());
};

function find(key: string) {
  const c = byKey(key);
  if (!c) throw new ApiError(404, `La raccolta «${key.trim()}» non esiste.`);
  return c;
}

export const listCollections = (): KbCollection[] => clone(collections.map(view));

export function createCollection(body: { name: string; description?: string }): KbCollection {
  const name = cleanName(body.name);
  const description = cleanDescription(body.description ?? "");
  if (collections.some((c) => c.name.toLowerCase() === name.toLowerCase())) throw new ApiError(409, `Esiste già una raccolta chiamata «${name}».`);
  const c = { id: id("kbk"), name, description, created_at: new Date().toISOString() };
  collections.push(c);
  return clone(view(c));
}

export function updateCollection(key: string, patch: { name?: string; description?: string }): KbCollection {
  const c = find(key);
  const name = patch.name === undefined ? c.name : cleanName(patch.name);
  const description = patch.description === undefined ? c.description : cleanDescription(patch.description);
  if (collections.some((o) => o.id !== c.id && o.name.toLowerCase() === name.toLowerCase())) throw new ApiError(409, `Esiste già una raccolta chiamata «${name}».`);
  c.name = name;
  c.description = description;
  return clone(view(c));
}

export function deleteCollection(key: string) {
  const c = find(key);
  collections.splice(collections.indexOf(c), 1);
  for (let i = docs.length - 1; i >= 0; i--) if (docs[i].collection_id === c.id) docs.splice(i, 1);
  for (const j of jobs) if (j.collection_id === c.id && j.state === "running") j.state = "cancelled";
}

export function listDocuments(key: string): KbDocument[] {
  const c = find(key);
  return clone(docs.filter((d) => d.collection_id === c.id).map(docView));
}

export function deleteDocument(docId: string) {
  const i = docs.findIndex((d) => d.id === docId);
  if (i < 0) throw new ApiError(404, `Il documento ${docId} non esiste.`);
  docs.splice(i, 1);
}

/** Paragraph-sized passages of about `CHUNK_TARGET` characters (a simplified `chunk_text`). */
export function chunk(text: string): string[] {
  const out: string[] = [];
  let cur = "";
  for (const para of text.split(/\n\s*\n/).map((p) => p.trim()).filter(Boolean)) {
    if (cur && cur.length + para.length + 2 > CHUNK_TARGET) {
      out.push(cur);
      cur = "";
    }
    if (para.length > CHUNK_TARGET) {
      for (let i = 0; i < para.length; i += CHUNK_TARGET) out.push(para.slice(i, i + CHUNK_TARGET));
    } else cur = cur ? `${cur}\n\n${para}` : para;
  }
  if (cur) out.push(cur);
  return out;
}

const extensionOf = (name: string) => (name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toLowerCase() : "");
const UNSUPPORTED = "tipo di file non supportato: si indicizzano solo testo, Markdown, codice e PDF";

function decode(data: string): string {
  try {
    const bin = atob(data);
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return "";
  }
}

const newJob = (colId: string, total: number): KbJob => ({ id: id("kbj"), collection_id: colId, state: "running", total, done: 0, added: 0, unchanged: 0, replaced: 0, failed: 0, skipped: [], skipped_total: 0 });

interface Item {
  name: string;
  source: string;
  mime: string;
  size: number;
  text: string | null;
}

/** The job finishes by itself a moment later, like the engine's background task does. */
function run(job: KbJob, items: Item[]) {
  const col = job.collection_id;
  setTimeout(() => {
    if (job.state !== "running") return;
    for (const it of items) {
      const hash = sha(it.text ?? it.name);
      const same = docs.find((d) => d.collection_id === col && d.sha256 === hash);
      if (same) {
        job.unchanged++;
      } else {
        const old = docs.find((d) => d.collection_id === col && d.source === it.source && it.source !== "caricato");
        if (old) {
          docs.splice(docs.indexOf(old), 1);
          job.replaced++;
        } else job.added++;
        const texts = it.text === null ? [`${it.name}: documento PDF (nella demo il testo non viene estratto).`] : chunk(it.text);
        const d = doc(col, it.name, it.source, it.size, 0, texts);
        d.sha256 = hash;
        docs.unshift(d);
      }
      job.done++;
    }
    job.current = undefined;
    job.state = "done";
  }, 700);
}

export function addFiles(key: string, files: AttachmentUpload[]): { job: KbJob } {
  const c = find(key);
  if (files.length === 0) throw new ApiError(400, "Scegli almeno un file.");
  if (files.length > MAX_ATTACHMENTS) throw new ApiError(400, `Puoi aggiungere al massimo ${MAX_ATTACHMENTS} file per volta.`);
  let total = 0;
  const items: Item[] = [];
  const skipped: KbJob["skipped"] = [];
  for (const f of files) {
    const size = Math.floor((f.data.replace(/=+$/, "").length * 3) / 4);
    if (size > MAX_ATTACHMENT_BYTES) throw new ApiError(400, `«${f.name}» è troppo grande: ogni file può pesare al massimo ${MAX_ATTACHMENT_BYTES >> 20} MB.`);
    total += size;
    const name = f.name.split(/[\\/]/).pop()?.trim() || "documento";
    const ext = extensionOf(name);
    if (size === 0) skipped.push({ name, reason: "il file è vuoto" });
    else if (ext === "pdf" || f.mime === "application/pdf") items.push({ name, source: "caricato", mime: "application/pdf", size, text: null });
    else if (f.mime.startsWith("text/") || TEXT_EXTENSIONS.has(ext)) {
      const text = decode(f.data);
      if (!text.trim()) skipped.push({ name, reason: "il file è vuoto" });
      else items.push({ name, source: "caricato", mime: f.mime || "text/plain", size, text });
    } else skipped.push({ name, reason: UNSUPPORTED });
  }
  if (total > MAX_ATTACHMENTS_TOTAL_BYTES) throw new ApiError(400, `I file pesano troppo: in totale al massimo ${MAX_ATTACHMENTS_TOTAL_BYTES >> 20} MB per volta.`);
  const job = newJob(c.id, items.length);
  job.skipped = skipped;
  job.skipped_total = skipped.length;
  jobs.unshift(job);
  run(job, items);
  return { job: clone(job) };
}

/** The demo has no disk: a folder is simulated with a few files, so the page can be tried. */
export function addPath(key: string, raw: string): { job: KbJob } {
  const c = find(key);
  const path = raw.trim();
  if (!path) throw new ApiError(400, "Indica il percorso di un file o di una cartella.");
  if (!path.startsWith("/") && !path.startsWith("~") && !/^[A-Za-z]:[\\/]/.test(path)) {
    throw new ApiError(400, `«${path}» non è un percorso assoluto: scrivilo per intero, ad esempio /home/nome/Documenti.`);
  }
  const base = path.replace(/[\\/]+$/, "");
  const items: Item[] = [
    { name: "appunti.md", source: `${base}/appunti.md`, mime: "text/markdown", size: 900, text: "# Appunti\n\nIdee per il progetto: raccogliere i documenti in una cartella e chiedere all'assistente di cercare tra le note." },
    { name: "elenco.txt", source: `${base}/elenco.txt`, mime: "text/plain", size: 400, text: "Elenco della spesa e delle cose da fare questa settimana: pane, latte, chiamare l'idraulico." },
  ];
  const job = newJob(c.id, items.length);
  job.skipped = [{ name: "foto.png", reason: UNSUPPORTED }];
  job.skipped_total = 1;
  jobs.unshift(job);
  run(job, items);
  return { job: clone(job) };
}

export function reindex(key: string): { job: KbJob } {
  const c = find(key);
  const mine = docs.filter((d) => d.collection_id === c.id);
  const job = newJob(c.id, mine.length);
  jobs.unshift(job);
  setTimeout(() => {
    if (job.state !== "running") return;
    job.done = job.total;
    job.unchanged = job.total;
    job.state = "done";
  }, 700);
  return { job: clone(job) };
}

export function cancelJob(jobId: string) {
  const j = jobs.find((x) => x.id === jobId);
  if (!j || j.state !== "running") throw new ApiError(404, "Nessuna operazione in corso con questo identificativo.");
  j.state = "cancelled";
}

export function status() {
  return clone({ ...(embeddingModel ? { embedding_model: embeddingModel } : {}), jobs: jobs.slice(0, 10) });
}

export function search(body: KbSearchBody): KbSearchResult {
  if (!body.query.trim()) throw new ApiError(400, "Scrivi cosa cercare.");
  const k = Math.min(Math.max(body.max_results ?? 5, 1), MAX_RESULTS);
  const col = body.collection?.trim() ? find(body.collection) : null;
  const q = tokenize(body.query);
  const hits: KbHit[] = [];
  for (const d of docs) {
    if (d.status !== "ready" || (col && d.collection_id !== col.id)) continue;
    d.texts.forEach((text, i) => {
      const words = tokenize(text);
      const n = q.filter((w) => words.some((t) => t.startsWith(w) || (t.length >= 4 && w.startsWith(t)))).length;
      if (n === 0) return;
      hits.push({
        chunk_id: `${d.id}_${i}`,
        document_id: d.id,
        document_name: d.name,
        collection_id: d.collection_id,
        collection_name: collections.find((c) => c.id === d.collection_id)?.name ?? "",
        part: i + 1,
        text,
        score: n,
      });
    });
  }
  hits.sort((a, b) => b.score - a.score);
  return { hits: hits.slice(0, k).map((h, i) => ({ ...h, score: 1 / (60 + i + 1) })), semantic: false };
}

/** Collection names the agent editor can offer, and the check of `save_agent`. */
export const collectionName = (key: string) => byKey(key)?.name;
export const collectionExists = (key: string) => collections.some((c) => c.id === key);
