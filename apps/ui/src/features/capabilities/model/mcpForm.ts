import type { McpSandbox, McpServer } from "../../../shared/api/types";
import { duplicateKeys, recordToRows, rowsToRecord, strayMasks, type KvRow } from "../../../shared/lib/kvRows";
import { isSecretName, SECRET_MASK } from "../../../shared/lib/secrets";
import { joinShellWords, splitShellWords } from "../../../shared/lib/tokens";

export type McpTransportKind = "stdio" | "http";

/** The drawer form: strings and rows while editing, converted to the API shape on save. */
export interface McpDraft {
  /** Present when an existing server is edited. */
  id?: string;
  name: string;
  transport: McpTransportKind;
  command: string;
  args: string[];
  env: KvRow[];
  url: string;
  headers: KvRow[];
  enabled: boolean;
  /** Sampling for this server; `undefined` = not set (off). */
  sampling?: boolean;
  /** Client id registered by hand (http servers without dynamic registration). */
  oauthClientId: string;
  /** Isolation of a stdio server (ignored for http). */
  sandbox: McpSandbox;
  /** Network for an isolated stdio server. */
  sandboxNetwork: boolean;
}

export type McpPayload = Partial<McpServer> & { name: string; transport: McpTransportKind };

/**
 * A new server is isolated (workspace-write, network on) when this system can do it; otherwise it stays `off`
 * and the form says why.
 */
export function emptyDraft(isolationAvailable = true): McpDraft {
  return {
    name: "", transport: "stdio", command: "", args: [], env: [], url: "", headers: [], enabled: true, oauthClientId: "",
    sandbox: isolationAvailable ? "workspace-write" : "off", sandboxNetwork: true,
  };
}

export const MCP_SANDBOX_LABEL: Record<McpSandbox, string> = {
  "off": "Nessun isolamento",
  "read-only": "Sola lettura",
  "workspace-write": "Scrittura nel workspace",
};

export function draftFromServer(s: McpServer): McpDraft {
  return {
    id: s.id,
    name: s.name,
    transport: s.transport,
    command: s.command ?? "",
    args: [...(s.args ?? [])],
    // The secrets come back as the mask: those rows are kept as they are unless the user changes them.
    env: recordToRows(s.env, SECRET_MASK),
    url: s.url ?? "",
    headers: recordToRows(s.headers, SECRET_MASK),
    enabled: s.enabled,
    sampling: s.sampling,
    oauthClientId: s.oauth_client_id ?? "",
    sandbox: s.sandbox ?? "off",
    sandboxNetwork: s.sandbox_network ?? true,
  };
}

/** Same rule as the engine: a server name has no spaces (they become `_`). */
export const normalizeName = (name: string) => name.trim().replace(/ /g, "_");

/**
 * Only the fields of the chosen transport are sent, so switching transport leaves nothing behind. A secret the user
 * did not touch goes back as `SECRET_MASK`, which the backend reads as "keep the stored value".
 */
export function draftToPayload(d: McpDraft): McpPayload {
  const base = { ...(d.id ? { id: d.id } : {}), name: normalizeName(d.name), enabled: d.enabled, ...(d.sampling !== undefined ? { sampling: d.sampling } : {}) };
  if (d.transport === "stdio") {
    const env = rowsToRecord(d.env);
    return { ...base, transport: "stdio", command: d.command.trim(), args: d.args, sandbox: d.sandbox, sandbox_network: d.sandboxNetwork, ...(Object.keys(env).length ? { env } : {}) };
  }
  const headers = rowsToRecord(d.headers);
  const clientId = d.oauthClientId.trim();
  return { ...base, transport: "http", url: d.url.trim(), ...(Object.keys(headers).length ? { headers } : {}), ...(clientId ? { oauth_client_id: clientId } : {}) };
}

export type McpErrors = Partial<Record<"name" | "command" | "url" | "env" | "headers", string>>;

/** An `http(s)` URL with a host, as the backend's `validate_mcp` requires. */
export function isHttpUrl(s: string): boolean {
  try {
    const u = new URL(s);
    return (u.protocol === "http:" || u.protocol === "https:") && u.hostname !== "";
  } catch {
    return false;
  }
}

const strayMessage = (names: string[]) =>
  `Il valore di ${names.join(", ")} è nascosto e non può passare a un altro nome: scrivilo di nuovo («${SECRET_MASK}» è riservato ai segreti già salvati).`;

/**
 * Mirrors `validate_mcp` (crates/aiw-engine/src/lib.rs): a name, a command for stdio, an `http(s)` address for
 * http. The backend says the same on a 400; this tells it before the request.
 * `others` are the names of the other servers (an edited server may keep its own name).
 */
export function validateDraft(d: McpDraft, others: string[]): McpErrors {
  const e: McpErrors = {};
  const name = normalizeName(d.name);
  if (!name) e.name = "Serve un nome";
  else if (others.some((n) => n.toLowerCase() === name.toLowerCase())) e.name = "Esiste già un server con questo nome";
  if (d.transport === "stdio") {
    if (!d.command.trim()) e.command = "Serve il comando da avviare";
    const dup = duplicateKeys(d.env);
    const stray = strayMasks(d.env, SECRET_MASK);
    if (dup.length) e.env = `Variabile ripetuta: ${dup.join(", ")}`;
    else if (stray.length) e.env = strayMessage(stray);
  } else {
    if (!d.url.trim()) e.url = "Serve l'indirizzo del server";
    else if (!isHttpUrl(d.url.trim())) e.url = "L'indirizzo deve iniziare con http:// o https://";
    const dup = duplicateKeys(d.headers, true);
    const stray = strayMasks(d.headers, SECRET_MASK);
    if (dup.length) e.headers = `Header ripetuto: ${dup.join(", ")}`;
    else if (stray.length) e.headers = strayMessage(stray);
  }
  return e;
}

/** Names whose value is a credential (the backend's rule): shown masked and never written in a list or a log. */
export const isSecretKey = (key: string): boolean => isSecretName(key.trim());

/** When the command field holds a whole command line, offers the split into command and arguments. */
export function splitCommandLine(d: Pick<McpDraft, "command" | "args">): { command: string; args: string[] } | null {
  if (!/\s/.test(d.command.trim())) return null;
  const words = splitShellWords(d.command);
  if (words.length < 2) return null;
  return { command: words[0], args: [...words.slice(1), ...d.args] };
}

/** One line that says what the server runs or where it connects. Header and env values never appear. */
export function serverTarget(s: Pick<McpServer, "transport" | "command" | "args" | "url">): string {
  if (s.transport === "http") return s.url ?? "";
  return joinShellWords([s.command ?? "", ...(s.args ?? [])].filter((w, i) => i > 0 || w !== ""));
}
