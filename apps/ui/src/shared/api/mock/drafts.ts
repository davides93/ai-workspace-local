import type { DraftAttachment, DraftStatus, EmailDraft, EmailDraftCreate, EmailDraftPatch, EmailDraftSend } from "../types";
import { ApiError } from "../http";
import { nid, now } from "./fixtures";
import { listChannels } from "./channels";

// Demo of the email drafts, with the rules the engine applies (crates/aiw-engine/src/channels/drafts.rs): a draft is
// reviewed by the owner and leaves only with the hash of the content they saw; one send per draft; a failure after the
// message was handed over is «uncertain»; refusing and cancelling never send; attachments come from the workspace only.
// Nothing is ever sent anywhere.

const STATUSES: DraftStatus[] = ["draft", "sending", "sent", "failed", "uncertain", "rejected", "cancelled"];
const MAX_RECIPIENTS = 20;
const MAX_ATTACHMENTS = 5;
const MAX_OPEN = 50;

const label: Record<DraftStatus, string> = { draft: "in attesa", sending: "in invio", sent: "inviata", failed: "non partita", uncertain: "esito incerto", rejected: "rifiutata", cancelled: "annullata" };

/** Not SHA-256: a stable 64-hex digest of the same fields, enough for the demo to refuse a stale approval. */
export function digest(text: string): string {
  let out = "";
  for (let round = 0; round < 8; round++) {
    let h = 0x811c9dc5 ^ (round * 0x9e3779b1);
    for (let i = 0; i < text.length; i++) {
      h ^= text.charCodeAt(i);
      h = Math.imul(h, 0x01000193);
      h ^= h >>> 15;
    }
    out += (h >>> 0).toString(16).padStart(8, "0");
  }
  return out;
}

const store: EmailDraft[] = [];
/** Messages the demo pretends to have sent (the tests read it: a refused draft must never be here). */
export const outbox: { id: string; to: string[]; subject: string }[] = [];

const hashOf = (d: EmailDraft) =>
  digest(JSON.stringify(["aiw-email-draft-v1", d.channel_id, d.from, d.to, d.cc, d.bcc, d.subject, d.body, d.in_reply_to ?? null, d.references ?? null, d.attachments.map((a) => [a.name, a.mime, a.size, a.sha256])]));

function seal(d: EmailDraft): EmailDraft {
  const ch = listChannels().find((c) => c.id === d.channel_id);
  d.from = typeof ch?.settings.from === "string" && ch.settings.from ? ch.settings.from : typeof ch?.settings.smtp_user === "string" ? ch.settings.smtp_user : d.from;
  d.content_hash = hashOf(d);
  return d;
}

function view(d: EmailDraft): EmailDraft {
  const out = structuredClone(seal(d));
  const twin = store.find((o) => o.id !== d.id && ["draft", "sending", "sent", "uncertain"].includes(o.status) && o.content_hash === out.content_hash);
  if (twin) out.duplicate_of = twin.id;
  else delete out.duplicate_of;
  return out;
}

function find(id: string): EmailDraft {
  const d = store.find((x) => x.id === id);
  if (!d) throw new ApiError(404, `bozza ${id} non trovata`);
  return d;
}

function addresses(list: string[] | undefined, what: string): string[] {
  const out: string[] = [];
  for (const raw of list ?? []) {
    const a = raw.trim();
    if (!a) continue;
    if (!/^[^@\s<>",;]+@[^@\s<>",;]+$/.test(a)) throw new ApiError(400, `«${a.slice(0, 60)}» non è un indirizzo email valido (${what})`);
    if (!out.some((x) => x.toLowerCase() === a.toLowerCase())) out.push(a);
  }
  return out;
}

function emailChannel(wanted?: string) {
  const all = listChannels().filter((c) => c.kind === "email");
  const ch = wanted ? all.find((c) => c.id === wanted || c.name.toLowerCase() === wanted.toLowerCase()) : all[0];
  if (!ch) throw new ApiError(wanted ? 404 : 400, wanted ? `canale email «${wanted}» non trovato` : "per preparare un'email serve un canale di tipo Email (Canali → Aggiungi un canale): è lui a conoscere il server di invio");
  return ch;
}

/** A path from the workspace becomes an attachment; anything that leaves it is refused, as the file tools do. */
function attach(paths: string[], files: DraftAttachment[]): void {
  for (const raw of paths) {
    const p = raw.trim();
    if (!p) continue;
    if (p.startsWith("/") || p.startsWith("~") || p.split("/").includes("..") || /^[a-z]:/i.test(p)) throw new ApiError(403, `${p} è fuori dal workspace`);
    const name = p.split("/").pop() ?? p;
    const sha = digest(`file:${p}`);
    if (files.some((f) => f.sha256 === sha)) continue;
    if (files.length >= MAX_ATTACHMENTS) throw new ApiError(400, `al massimo ${MAX_ATTACHMENTS} allegati`);
    const ext = name.split(".").pop()?.toLowerCase();
    const mime = ext === "pdf" ? "application/pdf" : ext === "txt" || ext === "md" ? "text/plain" : ext === "png" ? "image/png" : "application/octet-stream";
    files.push({ name, mime, size: 2048 + name.length * 37, sha256: sha, source: p });
  }
}

function text(subject: string, body: string): [string, string] {
  const s = subject.split(/\s+/).filter(Boolean).join(" ");
  if (s.length > 300) throw new ApiError(400, "l'oggetto è troppo lungo (al massimo 300 caratteri)");
  if (body.length > 100_000) throw new ApiError(400, "il testo è troppo lungo (al massimo 100000 caratteri)");
  return [s, body.replace(/\r\n/g, "\n")];
}

export function listDrafts(status?: DraftStatus): EmailDraft[] {
  if (status && !STATUSES.includes(status)) throw new ApiError(400, `stato non valido «${status}»: usa ${STATUSES.join(", ")}`);
  return store.filter((d) => !status || d.status === status).sort((a, b) => b.created_at.localeCompare(a.created_at)).map(view);
}

export function getDraft(id: string): EmailDraft {
  return view(find(id));
}

export function createDraft(c: EmailDraftCreate, by: "user" | "assistant" = "user"): EmailDraft {
  const ch = emailChannel(c.channel_id);
  const [subject, body] = text(c.subject ?? "", c.body ?? "");
  const d: EmailDraft = {
    id: nid("drf"), status: "draft", channel_id: ch.id, from: "", to: addresses(c.to, "a"), cc: addresses(c.cc, "cc"), bcc: addresses(c.bcc, "ccn"),
    subject, body, in_reply_to: c.in_reply_to || undefined, references: c.references || undefined, attachments: [],
    created_by: by, source: c.source?.trim() || by, content_hash: "", revision: 1, created_at: now(), updated_at: now(),
  };
  if (d.to.length + d.cc.length + d.bcc.length > MAX_RECIPIENTS) throw new ApiError(400, `al massimo ${MAX_RECIPIENTS} destinatari in tutto`);
  attach(c.attachments ?? [], d.attachments);
  if (store.filter((x) => x.status === "draft").length >= MAX_OPEN) throw new ApiError(400, `ci sono già ${MAX_OPEN} bozze in attesa: rifiutane o inviane qualcuna`);
  store.push(seal(d));
  return view(d);
}

export function updateDraft(id: string, p: EmailDraftPatch): EmailDraft {
  const d = find(id);
  if (d.status !== "draft" && d.status !== "failed") throw new ApiError(409, `la bozza è «${label[d.status]}»: non si può più modificare`);
  if (p.channel_id) d.channel_id = emailChannel(p.channel_id).id;
  if (p.to) d.to = addresses(p.to, "a");
  if (p.cc) d.cc = addresses(p.cc, "cc");
  if (p.bcc) d.bcc = addresses(p.bcc, "ccn");
  if (p.subject !== undefined || p.body !== undefined) [d.subject, d.body] = text(p.subject ?? d.subject, p.body ?? d.body);
  if (d.to.length + d.cc.length + d.bcc.length > MAX_RECIPIENTS) throw new ApiError(400, `al massimo ${MAX_RECIPIENTS} destinatari in tutto`);
  d.attachments = d.attachments.filter((a) => !(p.remove_attachments ?? []).includes(a.sha256));
  attach(p.add_attachments ?? [], d.attachments);
  Object.assign(d, { status: "draft", note: undefined, revision: d.revision + 1, updated_at: now() });
  seal(d);
  return view(d);
}

function move(d: EmailDraft, from: DraftStatus[], to: DraftStatus, extra: Partial<EmailDraft> = {}): EmailDraft {
  if (!from.includes(d.status)) {
    throw new ApiError(409, d.status === "sent" ? "la bozza è già stata inviata" : d.status === "sending" ? "la bozza è in corso di invio" : `la bozza è «${label[d.status]}»: l'operazione non è più possibile`);
  }
  Object.assign(d, { status: to, updated_at: now() }, extra);
  return d;
}

export function rejectDraft(id: string, reason?: string): EmailDraft {
  const note = (reason ?? "").split(/\s+/).filter(Boolean).join(" ").slice(0, 300) || undefined;
  return view(move(find(id), ["draft", "failed"], "rejected", { decided_at: now(), note }));
}

export function cancelDraft(id: string): EmailDraft {
  return view(move(find(id), ["draft", "failed"], "cancelled", { decided_at: now() }));
}

export function deleteDraft(id: string): void {
  const d = find(id);
  if (d.status === "draft" || d.status === "sending") throw new ApiError(409, "una bozza in attesa o in invio non si cancella: rifiutala o annullala prima");
  store.splice(store.indexOf(d), 1);
}

export function sendDraft(id: string, b: EmailDraftSend): EmailDraft {
  const d = find(id);
  if (d.status === "sent") return view(d);
  if (d.status === "sending") throw new ApiError(409, "la bozza è in corso di invio: aspetta l'esito");
  if (d.status === "rejected" || d.status === "cancelled") throw new ApiError(409, `la bozza è «${label[d.status]}»: non si invia`);
  if (d.status === "uncertain" && !b.resend) throw new ApiError(409, "non so se il messaggio è partito: controlla Posta inviata. Se vuoi spedirlo comunque, ripeti l'invio confermando che potrebbe arrivare due volte");
  const ch = listChannels().find((c) => c.id === d.channel_id);
  if (!ch || !ch.enabled) throw new ApiError(403, `il canale «${ch?.name ?? d.channel_id}» è spento: accendilo per inviare (l'interruttore d'emergenza ferma anche gli invii)`);
  seal(d);
  if (b.approved_hash.trim() !== d.content_hash) throw new ApiError(409, "la bozza è cambiata dopo la tua revisione (o il mittente del canale è cambiato): riguardala e approva il contenuto che vedi ora");
  if (d.to.length === 0) throw new ApiError(400, "manca il destinatario");
  if (!d.subject.trim() && !d.body.trim()) throw new ApiError(400, "la bozza è vuota");
  outbox.push({ id: d.id, to: [...d.to, ...d.cc, ...d.bcc], subject: d.subject });
  return view(move(d, ["draft", "failed", "uncertain"], "sent", { sent_at: now(), decided_at: now(), note: undefined, message_id: `<draft-${d.id}@aiw.local>` }));
}

/** For the scenarios and the tests: put a draft in a given state without going through the API. */
export function seedDraft(c: EmailDraftCreate, by: "user" | "assistant", state: Partial<EmailDraft> = {}): EmailDraft {
  const d = createDraft(c, by);
  Object.assign(find(d.id), state);
  return view(find(d.id));
}

export function resetDrafts(): void {
  store.length = 0;
  outbox.length = 0;
}
