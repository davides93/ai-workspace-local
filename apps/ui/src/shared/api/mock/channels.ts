import type { Channel, ChannelCreate, ChannelInfo, ChannelKind, ChannelPairing, ChannelPatch, ChannelStatus, ChannelTestResult, MailAccount, MailAccountTest } from "../types";
import { SECRET_MASK } from "../types";
import { ApiError } from "../http";
import { agents } from "./fixtures";

// Demo of the channels and of the IMAP accounts, with the rules the engine applies
// (crates/aiw-engine/src/channels/mod.rs, mail_accounts.rs). Nothing connects anywhere.

const KINDS: ChannelKind[] = ["telegram", "slack", "email"];
const SECRETS: Record<ChannelKind, string[]> = { telegram: ["bot_token"], slack: ["bot_token", "app_token"], email: ["imap_password", "smtp_password"] };
const KIND_LABEL: Record<ChannelKind, string> = { telegram: "Telegram", slack: "Slack", email: "Email" };
const MAX_ACCOUNTS = 5;

let counter = 0;
const nid = (p: string) => `${p}_${Date.now().toString(36)}${(counter++).toString(36)}`;

interface Stored {
  channel: Channel;
  status: ChannelStatus;
}
const store: Stored[] = [];
const accounts: MailAccount[] = [];
const pairs = new Map<string, { code: string; expires: number }>();

const off = (): ChannelStatus => ({ state: "off", detail: "Spento: non si collega a nulla.", ignored: 0, handled: 0, conversations: 0 });

/** Secrets are never shown: the mask where one is stored, nothing where it is not. */
function masked(c: Channel): Channel {
  const secrets: Record<string, string> = {};
  for (const k of SECRETS[c.kind]) secrets[k] = c.secrets[k] ? SECRET_MASK : "";
  return { ...c, secrets, allowed: [...c.allowed], settings: { ...c.settings } };
}

const info = (s: Stored): ChannelInfo => ({ ...masked(s.channel), status: { ...s.status }, downgraded: false });

function find(id: string): Stored {
  const s = store.find((x) => x.channel.id === id);
  if (!s) throw new ApiError(404, `canale ${id} non trovato`);
  return s;
}

/** Same lists as the engine: lower-cased addresses, `@` stripped from Telegram handles, no duplicates. */
export function normaliseAllowed(kind: ChannelKind, list: string[]): string[] {
  const out: string[] = [];
  for (const raw of list) {
    let a = raw.trim();
    if (kind === "email") a = a.toLowerCase();
    if (kind === "telegram") a = a.replace(/^@/, "");
    if (a && !out.includes(a)) out.push(a);
  }
  return out;
}

function problem(c: Channel): string | null {
  if (c.name.length > 60) return "il nome del canale è troppo lungo (al massimo 60 caratteri)";
  if (c.mode !== "read_only" && c.mode !== "ask") return "la modalità del canale è «read_only» (solo lettura) oppure «ask» (chiede conferma)";
  if (!agents.some((a) => a.id === c.agent_id)) return `l'assistente «${c.agent_id}» non esiste`;
  if (c.kind === "slack") {
    const bad = c.allowed.find((a) => !/^[UW][A-Z0-9]{6,}$/.test(a));
    if (bad) return `«${bad}» non è un identificativo Slack: inizia con U o W (per esempio U024BE7LH)`;
  }
  if (c.kind === "email") {
    const bad = c.allowed.find((a) => !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(a));
    if (bad) return `«${bad}» non è un indirizzo email`;
  }
  return null;
}

function notReady(c: Channel): string | null {
  const has = (k: string) => (c.secrets[k] ?? "").trim() !== "";
  const set = (k: string) => typeof c.settings[k] === "string" && (c.settings[k] as string).trim() !== "";
  if (c.kind === "telegram" && !has("bot_token")) return "Manca il token del bot (lo dà @BotFather).";
  if (c.kind === "slack" && (!has("bot_token") || !has("app_token"))) return "Mancano il token del bot (xoxb-…) e quello dell'app (xapp-…).";
  if (c.kind === "email" && (!set("imap_host") || !set("imap_user") || !has("imap_password"))) return "Mancano il server, l'utente o la password della posta in arrivo.";
  if (c.kind === "email" && !set("smtp_host")) return "Manca il server di invio (SMTP).";
  return null;
}

export function listChannels(): ChannelInfo[] {
  return store.map(info);
}

export function createChannel(b: ChannelCreate): ChannelInfo {
  if (!KINDS.includes(b.kind)) throw new ApiError(400, `tipo di canale sconosciuto: «${String(b.kind)}» (telegram, slack o email)`);
  const secrets: Record<string, string> = {};
  for (const [k, v] of Object.entries(b.secrets ?? {})) if (SECRETS[b.kind].includes(k) && v !== SECRET_MASK) secrets[k] = v;
  const channel: Channel = {
    id: nid("chn"),
    kind: b.kind,
    name: (b.name ?? "").split(/\s+/).filter(Boolean).join(" ") || KIND_LABEL[b.kind],
    enabled: false,
    agent_id: b.agent_id ?? agents.find((a) => a.id === "operator")?.id ?? agents[0]?.id ?? "",
    mode: b.mode ?? "read_only",
    allowed: normaliseAllowed(b.kind, b.allowed ?? []),
    briefing: false,
    secrets,
    settings: { ...(b.settings ?? {}) },
  };
  const bad = problem(channel);
  if (bad) throw new ApiError(400, bad);
  const s = { channel, status: off() };
  store.push(s);
  return info(s);
}

export function updateChannel(id: string, p: ChannelPatch): ChannelInfo {
  const s = find(id);
  const c: Channel = { ...s.channel, secrets: { ...s.channel.secrets }, settings: { ...s.channel.settings } };
  if (p.name !== undefined) c.name = p.name.split(/\s+/).filter(Boolean).join(" ") || KIND_LABEL[c.kind];
  if (p.agent_id !== undefined) c.agent_id = p.agent_id;
  if (p.mode !== undefined) c.mode = p.mode;
  if (p.allowed !== undefined) c.allowed = normaliseAllowed(c.kind, p.allowed);
  if (p.briefing !== undefined) c.briefing = p.briefing;
  for (const [k, v] of Object.entries(p.secrets ?? {})) if (v !== SECRET_MASK) c.secrets[k] = v;
  Object.assign(c.settings, p.settings ?? {});
  if (p.enabled !== undefined) c.enabled = p.enabled;
  const bad = problem(c);
  if (bad) throw new ApiError(400, bad);
  if (c.enabled) {
    const missing = notReady(c);
    if (missing) throw new ApiError(400, missing);
    if (c.allowed.length === 0 && c.kind !== "telegram") throw new ApiError(400, "prima di accenderlo scrivi chi può parlare con l'assistente: senza lista nessuno viene ascoltato");
  }
  s.channel = c;
  s.status = c.enabled
    ? { ...s.status, state: "connected", detail: "Demo: nessuna connessione reale.", since: new Date().toISOString() }
    : { ...s.status, state: "off", detail: "Spento: non si collega a nulla.", since: undefined };
  return info(s);
}

export function deleteChannel(id: string): void {
  const i = store.findIndex((x) => x.channel.id === id);
  if (i < 0) throw new ApiError(404, `canale ${id} non trovato`);
  store.splice(i, 1);
  pairs.delete(id);
}

export function channelStatus(id: string): ChannelStatus {
  return { ...find(id).status };
}

export function testChannel(id: string): ChannelTestResult {
  const c = find(id).channel;
  const missing = notReady(c);
  if (missing) return { ok: false, detail: missing };
  return { ok: true, detail: `Demo: il canale ${KIND_LABEL[c.kind]} sembra a posto (nessuna connessione reale).` };
}

export function pairChannel(id: string): ChannelPairing {
  const c = find(id).channel;
  if (c.kind !== "telegram") throw new ApiError(400, "l'abbinamento con un codice c'è solo per Telegram: negli altri canali scrivi l'identificativo nella lista");
  const code = String(Math.floor(100000 + Math.random() * 900000));
  const expires = Date.now() + 10 * 60_000;
  pairs.set(id, { code, expires });
  return { code, expires_at: new Date(expires).toISOString(), command: `/start ${code}` };
}

// ---- IMAP accounts of the mail connector

const slug = (s: string) => s.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");

export function listMailAccounts(): MailAccount[] {
  return accounts.map((a) => ({ ...a, folders: [...a.folders] }));
}

export function setMailAccounts(list: MailAccount[]): MailAccount[] {
  if (list.length > MAX_ACCOUNTS) throw new ApiError(400, `al massimo ${MAX_ACCOUNTS} account di posta`);
  const next: MailAccount[] = [];
  for (const a of list) {
    const host = a.host.trim();
    const username = a.username.trim();
    if (!host || !username) throw new ApiError(400, "Compila il server e il nome utente.");
    const id = slug(a.id || username || host) || nid("m");
    if (next.some((x) => x.id === id)) throw new ApiError(400, `due account hanno lo stesso nome «${id}»`);
    const old = accounts.find((x) => x.id === id);
    const password = a.password === SECRET_MASK ? old?.password ?? "" : a.password;
    next.push({ id, host, port: a.port || 993, username, password, folders: a.folders.map((f) => f.trim()).filter(Boolean).length ? a.folders.map((f) => f.trim()).filter(Boolean) : ["INBOX"] });
  }
  accounts.splice(0, accounts.length, ...next);
  return listMailAccounts().map((a) => ({ ...a, password: a.password ? SECRET_MASK : "" }));
}

export function listMaskedMailAccounts(): MailAccount[] {
  return listMailAccounts().map((a) => ({ ...a, password: a.password ? SECRET_MASK : "" }));
}

export function testMailAccount(a: MailAccount): MailAccountTest {
  if (!a.host.trim() || !a.username.trim()) return { ok: false, detail: "Compila il server e il nome utente.", folders: [] };
  if (!a.password) return { ok: false, detail: "Manca la password (per Gmail e Microsoft serve una «password per le app»).", folders: [] };
  const folders = (a.folders.length ? a.folders : ["INBOX"]).map((name) => ({ name, recent: 3 }));
  return { ok: true, detail: `Demo: collegato, ${folders.length * 3} messaggi negli ultimi 7 giorni. Non ho letto né cambiato nulla.`, folders };
}
