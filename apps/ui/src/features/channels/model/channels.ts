import type { ChannelInfo, ChannelKind, ChannelMode, ChannelPatch, ChannelState, MailAccount } from "../../../shared/api/types";
import { SECRET_MASK } from "../../../shared/api/types";

export interface SecretField {
  key: string;
  label: string;
  placeholder: string;
}

export interface SettingField {
  key: string;
  label: string;
  placeholder: string;
  hint?: string;
}

export interface KindInfo {
  id: ChannelKind;
  label: string;
  /** One line that says what it is, in plain words. */
  blurb: string;
  /** The guided set-up, one step per line. */
  steps: string[];
  secrets: SecretField[];
  settings: SettingField[];
  allowedLabel: string;
  allowedHint: string;
  allowedPlaceholder: string;
}

export const CHANNEL_KINDS: KindInfo[] = [
  {
    id: "telegram",
    label: "Telegram",
    blurb: "Scrivi all'assistente da Telegram, come a un amico.",
    steps: [
      "Apri Telegram e scrivi a @BotFather il comando /newbot.",
      "Scegli un nome e un indirizzo per il tuo bot: @BotFather ti risponde con un «token».",
      "Incolla qui sotto il token e salva.",
      "Premi «Genera il codice» e scrivi al tuo bot il messaggio che compare: da quel momento il bot ti riconosce.",
      "Accendi il canale.",
    ],
    secrets: [{ key: "bot_token", label: "Token del bot", placeholder: "123456:ABC-DEF…" }],
    settings: [],
    allowedLabel: "Chi può scrivere",
    allowedHint: "Il numero di Telegram di chi può parlare con l'assistente. Di solito basta il codice qui sotto: aggiunge te in automatico. Chiunque altro viene ignorato.",
    allowedPlaceholder: "Numero (per esempio 123456789)",
  },
  {
    id: "slack",
    label: "Slack",
    blurb: "Scrivi all'assistente in un messaggio privato o citandolo in un canale.",
    steps: [
      "Su api.slack.com/apps crea una nuova app «From scratch» nel tuo spazio di lavoro.",
      "In «Socket Mode» attiva Socket Mode e crea un token a livello di app con il permesso connections:write: inizia con xapp-.",
      "In «OAuth & Permissions» aggiungi i permessi del bot chat:write, im:history, app_mentions:read, users:read e installa l'app: il token del bot inizia con xoxb-.",
      "In «Event Subscriptions» attiva gli eventi message.im e app_mention.",
      "In «App Home» consenti i messaggi al bot. Incolla qui i due token, scrivi chi può parlare e accendi il canale.",
    ],
    secrets: [
      { key: "app_token", label: "Token dell'app (xapp-…)", placeholder: "xapp-1-…" },
      { key: "bot_token", label: "Token del bot (xoxb-…)", placeholder: "xoxb-…" },
    ],
    settings: [],
    allowedLabel: "Chi può scrivere",
    allowedHint: "Gli identificativi Slack delle persone (iniziano con U o W): su Slack, profilo → «Copia ID membro». Chiunque altro viene ignorato.",
    allowedPlaceholder: "Per esempio U024BE7LH",
  },
  {
    id: "email",
    label: "Email",
    blurb: "Scrivi un'email all'assistente e ricevi la risposta nella stessa conversazione.",
    steps: [
      "Crea una casella dedicata all'assistente (non la tua posta personale).",
      "Per Gmail e Microsoft serve una «password per le app»: l'accesso con l'account normale (OAuth) non c'è ancora.",
      "Scrivi i server di posta in arrivo (IMAP) e in uscita (SMTP) e le password.",
      "Scrivi gli indirizzi che possono scrivere all'assistente: la posta di chiunque altro viene ignorata.",
      "Accendi il canale e premi «Prova».",
    ],
    secrets: [
      { key: "imap_password", label: "Password della posta in arrivo", placeholder: "password per le app" },
      { key: "smtp_password", label: "Password della posta in uscita", placeholder: "password per le app" },
    ],
    settings: [
      { key: "imap_host", label: "Server IMAP", placeholder: "imap.example.com" },
      { key: "imap_user", label: "Utente IMAP", placeholder: "assistente@example.com" },
      { key: "smtp_host", label: "Server SMTP", placeholder: "smtp.example.com" },
      { key: "smtp_user", label: "Utente SMTP", placeholder: "come l'utente IMAP, se vuoto" },
      { key: "from", label: "Indirizzo da cui rispondere", placeholder: "assistente@example.com" },
    ],
    allowedLabel: "Indirizzi che possono scrivere",
    allowedHint: "Solo la posta da questi indirizzi, e solo se il server di posta ne garantisce l'autenticità (DKIM o SPF), arriva all'assistente.",
    allowedPlaceholder: "nome@example.com",
  },
];

export const kindInfo = (kind: ChannelKind): KindInfo => CHANNEL_KINDS.find((k) => k.id === kind) ?? CHANNEL_KINDS[0];

export const MODE_LABEL: Record<ChannelMode, string> = { read_only: "Solo lettura", ask: "Chiede conferma" };

export const MODE_NOTE: Record<ChannelMode, string> = {
  read_only: "L'assistente può rispondere e consultare, ma non scrive file, non esegue comandi e non chiede nulla.",
  ask: "L'assistente può agire, ma ogni azione delicata chiede conferma: nell'app e nel canale stesso (sì o no). Con un assistente in modalità automatica o «yolo» le conferme restano comunque attive.",
};

export const DOWNGRADE_NOTE = "L'assistente scelto ha la modalità automatica: nei canali non viene mai rispettata, e chiede conferma come negli altri casi.";

export const STATE_LABEL: Record<ChannelState, string> = {
  off: "Spento",
  connecting: "Si sta collegando",
  connected: "Connesso",
  error: "Errore",
  unavailable: "Non disponibile",
};

export type StateTone = "ok" | "run" | "crit" | "off" | "warn";
export const stateTone = (s: ChannelState): StateTone => (s === "connected" ? "ok" : s === "connecting" ? "run" : s === "error" ? "crit" : s === "unavailable" ? "warn" : "off");

export const ALWAYS_ON_NOTE = "I canali funzionano solo mentre l'app (o il servizio sul computer) è acceso. Sul telefono non lavorano in background.";

/** Which secrets still need a value before the channel can be turned on. */
export function missingSecrets(channel: Pick<ChannelInfo, "kind" | "secrets">): string[] {
  return kindInfo(channel.kind).secrets.filter((f) => !channel.secrets[f.key]).map((f) => f.label);
}

/** Why the channel cannot be turned on yet, in plain words; null when it can. */
export function enableProblem(channel: ChannelInfo): string | null {
  const missing = missingSecrets(channel);
  if (missing.length > 0) return `Manca: ${missing.join(", ")}.`;
  if (channel.kind === "email") {
    const s = channel.settings;
    const has = (k: string) => typeof s[k] === "string" && (s[k] as string).trim() !== "";
    if (!has("imap_host") || !has("imap_user")) return "Mancano il server e l'utente della posta in arrivo.";
    if (!has("smtp_host")) return "Manca il server della posta in uscita.";
  }
  if (channel.kind !== "telegram" && channel.allowed.length === 0) return "Scrivi chi può parlare con l'assistente.";
  return null;
}

/** The sentence under the counters: how many messages from strangers were dropped. */
export function ignoredNote(n: number): string {
  if (n === 0) return "Nessun messaggio ignorato.";
  return n === 1 ? "1 messaggio ignorato da sconosciuti." : `${n} messaggi ignorati da sconosciuti.`;
}

/** The body of a save: only the secrets the person actually typed (an untouched field stays masked and is kept). */
export function secretsToSend(typed: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(typed)) if (v.trim() !== "") out[k] = v.trim();
  return out;
}

/** What changed between the stored channel and the form, as a PATCH body (empty when nothing changed). */
export function diffPatch(
  stored: ChannelInfo,
  form: { name: string; mode: ChannelMode; allowed: string[]; settings: Record<string, string>; typedSecrets: Record<string, string> },
): ChannelPatch {
  const patch: ChannelPatch = {};
  if (form.name.trim() !== stored.name) patch.name = form.name.trim();
  if (form.mode !== stored.mode) patch.mode = form.mode;
  if (form.allowed.join("\n") !== stored.allowed.join("\n")) patch.allowed = form.allowed;
  const settings: Record<string, string> = {};
  for (const f of kindInfo(stored.kind).settings) {
    const before = typeof stored.settings[f.key] === "string" ? (stored.settings[f.key] as string) : "";
    if ((form.settings[f.key] ?? "") !== before) settings[f.key] = (form.settings[f.key] ?? "").trim();
  }
  if (Object.keys(settings).length > 0) patch.settings = settings;
  const secrets = secretsToSend(form.typedSecrets);
  if (Object.keys(secrets).length > 0) patch.secrets = secrets;
  return patch;
}

/** The pairing code is valid ten minutes: how many whole minutes are left (0 when over). */
export function minutesLeft(expiresAt: string, now = Date.now()): number {
  const ms = Date.parse(expiresAt) - now;
  return Number.isFinite(ms) && ms > 0 ? Math.ceil(ms / 60_000) : 0;
}

// ---- IMAP accounts of the mail connector

export const MAIL_ACCOUNT_LIMIT = 5;

export const emptyAccount = (): MailAccount => ({ id: "", host: "", port: 993, username: "", password: "", folders: ["INBOX"] });

/** Plain-words problem with an account form; null when it can be saved. */
export function accountProblem(a: MailAccount): string | null {
  if (!a.host.trim()) return "Scrivi il server (per esempio imap.example.com).";
  if (!a.username.trim()) return "Scrivi il nome utente.";
  if (!Number.isInteger(a.port) || a.port < 1 || a.port > 65535) return "La porta è un numero tra 1 e 65535 (di solito 993).";
  if (!a.password) return "Scrivi la password (per Gmail e Microsoft serve una «password per le app»).";
  return null;
}

export const hasStoredPassword = (a: MailAccount): boolean => a.password === SECRET_MASK;

/** Folders typed one per line or separated by commas. */
export function parseFolders(raw: string): string[] {
  return raw.split(/[\n,]/).map((f) => f.trim()).filter(Boolean);
}
