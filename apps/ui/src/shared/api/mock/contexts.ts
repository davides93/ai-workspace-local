import type { ChannelInfo, ChannelKind, ContextCreate, ContextInfo, ContextMember, ContextPatch } from "../types";
import { ApiError } from "../http";
import { nid, now, sessions } from "./fixtures";
import { listChannels, normaliseAllowed } from "./channels";

// Demo of the shared contexts, with the rules the engine applies (crates/aiw-engine/src/channels/context.rs):
// the owner links accounts explicitly, one per channel, each on the list of its channel and in one context at most,
// all with the same assistant; a Telegram group is not a person; an email channel that does not check the sender cannot
// take part; the context is never freer than its most restricted channel. Nothing is ever merged by itself.

const MAX_MEMBERS = 8;
const MAX_CONTEXTS = 20;

interface Stored {
  id: string;
  name: string;
  session_id: string;
  members: ContextMember[];
  created_at: string;
}
const store: Stored[] = [];

const normSender = (kind: ChannelKind, s: string) => normaliseAllowed(kind, [s])[0] ?? "";

function problemOf(ch: ChannelInfo | undefined, sender: string): string | undefined {
  if (!ch) return "il canale non esiste più";
  if (!ch.allowed.includes(normSender(ch.kind, sender))) return "non è più nella lista di chi può parlare con il canale";
  if (ch.kind === "telegram" && !(Number(sender) > 0)) return "serve l'identificativo di una persona: quello di un gruppo non si collega";
  if (ch.kind === "email" && ch.settings.require_auth === false) {
    return "il controllo del mittente è spento su questo canale: un mittente falsificato leggerebbe la cronologia degli altri canali";
  }
  return undefined;
}

/** Takes out the accounts that can no longer be there (the engine does it when a channel changes). */
function prune(): void {
  const channels = listChannels();
  for (const c of store) {
    c.members = c.members.filter((m) => {
      const ch = channels.find((x) => x.id === m.channel_id);
      return !!ch && ch.allowed.includes(normSender(ch.kind, m.sender));
    });
  }
  for (let i = store.length - 1; i >= 0; i--) if (store[i].members.length === 0) store.splice(i, 1);
}

function info(c: Stored): ContextInfo {
  const channels = listChannels();
  const chans = c.members.map((m) => channels.find((x) => x.id === m.channel_id)).filter((x): x is ChannelInfo => !!x);
  const session = sessions.find((s) => s.id === c.session_id);
  return {
    id: c.id,
    name: c.name,
    agent_id: chans[0]?.agent_id ?? "operator",
    session_id: c.session_id,
    members: c.members.map((m) => ({ ...m })),
    created_at: c.created_at,
    mode: chans.length > 0 && chans.every((x) => x.mode === "ask") ? "ask" : "read_only",
    session_title: session?.title ?? "",
    members_info: c.members.map((m) => {
      const ch = channels.find((x) => x.id === m.channel_id);
      return { channel_id: m.channel_id, channel_name: ch?.name ?? "", kind: ch?.kind ?? "telegram", sender: m.sender, problem: problemOf(ch, m.sender) };
    }),
  };
}

function check(members: ContextMember[], except?: string): ContextMember[] {
  if (members.length === 0) throw new ApiError(400, "un contesto condiviso ha bisogno di almeno un account (canale e mittente)");
  if (members.length > MAX_MEMBERS) throw new ApiError(400, `al massimo ${MAX_MEMBERS} account per contesto`);
  const channels = listChannels();
  const clean: ContextMember[] = [];
  for (const m of members) {
    const ch = channels.find((x) => x.id === m.channel_id.trim());
    if (!ch) throw new ApiError(400, `il canale «${m.channel_id}» non esiste`);
    const sender = normSender(ch.kind, m.sender);
    if (clean.some((c) => c.channel_id === ch.id)) throw new ApiError(400, `il canale «${ch.name}» compare due volte: in un contesto c'è un solo account per canale`);
    if (!ch.allowed.includes(sender)) throw new ApiError(400, `«${sender}» non è nella lista del canale «${ch.name}»: aggiungilo prima a chi può parlare con l'assistente`);
    const why = problemOf(ch, sender);
    if (why) throw new ApiError(400, `«${sender}» su «${ch.name}»: ${why}`);
    const other = store.find((c) => c.id !== except && c.members.some((x) => x.channel_id === ch.id && x.sender === sender));
    if (other) throw new ApiError(409, `«${sender}» su «${ch.name}» fa già parte del contesto «${other.name}»`);
    clean.push({ channel_id: ch.id, sender });
  }
  const first = channels.find((x) => x.id === clean[0].channel_id);
  const diff = clean.map((m) => channels.find((x) => x.id === m.channel_id)).find((x) => x && first && x.agent_id !== first.agent_id);
  if (first && diff) {
    throw new ApiError(400, `i canali usano assistenti diversi («${first.name}» usa «${first.agent_id}», «${diff.name}» usa «${diff.agent_id}»): imposta lo stesso assistente su tutti, altrimenti le risposte cambierebbero da un canale all'altro`);
  }
  return clean;
}

const cleanName = (raw: string) => {
  const name = raw.split(/\s+/).filter(Boolean).join(" ");
  if (!name) throw new ApiError(400, "il contesto ha bisogno di un nome");
  if (name.length > 60) throw new ApiError(400, "il nome del contesto è troppo lungo (al massimo 60 caratteri)");
  return name;
};

export function listContexts(): ContextInfo[] {
  prune();
  return store.map(info);
}

export function createContext(b: ContextCreate): ContextInfo {
  prune();
  const name = cleanName(b.name);
  if (store.length >= MAX_CONTEXTS) throw new ApiError(400, `al massimo ${MAX_CONTEXTS} contesti condivisi`);
  const members = check(b.members);
  const session_id = nid("ses");
  const draft: Stored = { id: nid("ctx"), name, session_id, members, created_at: now() };
  const mode = info(draft).mode === "ask" ? "ask" : "ro";
  sessions.unshift({ id: session_id, title: `Conversazione condivisa · ${name}`, agent_id: `channel:${mode}:${info(draft).agent_id}`, created_at: now(), updated_at: now(), channel: "shared" });
  store.push(draft);
  return info(draft);
}

export function updateContext(id: string, p: ContextPatch): ContextInfo {
  prune();
  const c = store.find((x) => x.id === id);
  if (!c) throw new ApiError(404, `contesto condiviso ${id} non trovato`);
  if (p.name !== undefined) c.name = cleanName(p.name);
  if (p.members !== undefined) c.members = check(p.members, id);
  return info(c);
}

export function deleteContext(id: string): void {
  const i = store.findIndex((x) => x.id === id);
  if (i < 0) throw new ApiError(404, `contesto condiviso ${id} non trovato`);
  store.splice(i, 1);
}

/** For the scenarios and the tests: forget everything. */
export function resetContexts(): void {
  store.length = 0;
}
