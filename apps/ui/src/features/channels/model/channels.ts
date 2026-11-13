import type { ChannelInfo, ChannelKind, ChannelMode, ChannelPatch, ChannelState, MailAccount } from "../../../shared/api/types";
import { SECRET_MASK } from "../../../shared/api/types";

import { t } from "../../../shared/i18n";
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
    get blurb() { return t("channels.scrivi_all_assistente_da_telegram"); },
    steps: [
      "Apri Telegram e scrivi a @BotFather il comando /newbot.",
      "Scegli un nome e un indirizzo per il tuo bot: @BotFather ti risponde con un «token».",
      "Incolla qui sotto il token e salva.",
      "Premi «Genera il codice» e scrivi al tuo bot il messaggio che compare: da quel momento il bot ti riconosce.",
      "Accendi il canale.",
    ],
    secrets: [{ key: "bot_token", get label() { return t("channels.token_del_bot"); }, placeholder: "123456:ABC-DEF…" }],
    settings: [],
    get allowedLabel() { return t("channels.chi_puo_scrivere"); },
    get allowedHint() { return t("channels.il_numero_di_telegram_di"); },
    get allowedPlaceholder() { return t("channels.numero_per_esempio_123456789"); },
  },
  {
    id: "slack",
    label: "Slack",
    get blurb() { return t("channels.scrivi_all_assistente_in_un"); },
    steps: [
      "Su api.slack.com/apps crea una nuova app «From scratch» nel tuo spazio di lavoro.",
      "In «Socket Mode» attiva Socket Mode e crea un token a livello di app con il permesso connections:write: inizia con xapp-.",
      "In «OAuth & Permissions» aggiungi i permessi del bot chat:write, im:history, app_mentions:read, users:read e installa l'app: il token del bot inizia con xoxb-.",
      "In «Event Subscriptions» attiva gli eventi message.im e app_mention.",
      "In «App Home» consenti i messaggi al bot. Incolla qui i due token, scrivi chi può parlare e accendi il canale.",
    ],
    secrets: [
      { key: "app_token", get label() { return t("channels.token_dell_app_xapp"); }, placeholder: "xapp-1-…" },
      { key: "bot_token", get label() { return t("channels.token_del_bot_xoxb"); }, placeholder: "xoxb-…" },
    ],
    settings: [],
    get allowedLabel() { return t("channels.chi_puo_scrivere"); },
    get allowedHint() { return t("channels.gli_identificativi_slack_delle_persone"); },
    allowedPlaceholder: "Per esempio U024BE7LH",
  },
  {
    id: "email",
    label: "Email",
    get blurb() { return t("channels.scrivi_un_email_all_assistente"); },
    steps: [
      "Crea una casella dedicata all'assistente (non la tua posta personale).",
      "Per Gmail e Microsoft serve una «password per le app»: l'accesso con l'account normale (OAuth) non c'è ancora.",
      "Scrivi i server di posta in arrivo (IMAP) e in uscita (SMTP) e le password.",
      "Scrivi gli indirizzi che possono scrivere all'assistente: la posta di chiunque altro viene ignorata.",
      "Accendi il canale e premi «Prova».",
    ],
    secrets: [
      { key: "imap_password", get label() { return t("channels.password_della_posta_in_arrivo"); }, get placeholder() { return t("channels.password_per_le_app"); } },
      { key: "smtp_password", get label() { return t("channels.password_della_posta_in_uscita"); }, get placeholder() { return t("channels.password_per_le_app"); } },
    ],
    settings: [
      { key: "imap_host", get label() { return t("channels.server_imap"); }, placeholder: "imap.example.com" },
      { key: "imap_user", get label() { return t("channels.utente_imap"); }, placeholder: "assistente@example.com" },
      { key: "smtp_host", get label() { return t("channels.server_smtp"); }, placeholder: "smtp.example.com" },
      { key: "smtp_user", get label() { return t("channels.utente_smtp"); }, get placeholder() { return t("channels.come_l_utente_imap_se"); } },
      { key: "from", get label() { return t("channels.indirizzo_da_cui_rispondere"); }, placeholder: "assistente@example.com" },
    ],
    get allowedLabel() { return t("channels.indirizzi_che_possono_scrivere"); },
    get allowedHint() { return t("channels.solo_la_posta_da_questi"); },
    allowedPlaceholder: "nome@example.com",
  },
];

export const kindInfo = (kind: ChannelKind): KindInfo => CHANNEL_KINDS.find((k) => k.id === kind) ?? CHANNEL_KINDS[0];

export const MODE_LABEL: Record<ChannelMode, string> = { get read_only() { return t("channels.solo_lettura"); }, get ask() { return t("channels.chiede_conferma"); } };

export const MODE_NOTE: Record<ChannelMode, string> = {
  get read_only() { return t("channels.l_assistente_puo_rispondere_e"); },
  get ask() { return t("channels.l_assistente_puo_agire_ma"); },
};

export const DOWNGRADE_NOTE = "L'assistente scelto ha la modalità automatica: nei canali non viene mai rispettata, e chiede conferma come negli altri casi.";

export const STATE_LABEL: Record<ChannelState, string> = {
  get off() { return t("channels.spento"); },
  connecting: "Si sta collegando",
  get connected() { return t("channels.connesso"); },
  get error() { return t("channels.errore"); },
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
    if (!has("imap_host") || !has("imap_user")) return t("channels.mancano_il_server_e_l");
    if (!has("smtp_host")) return t("channels.manca_il_server_della_posta");
  }
  if (channel.kind !== "telegram" && channel.allowed.length === 0) return t("channels.scrivi_chi_puo_parlare_con");
  return null;
}

/** The sentence under the counters: how many messages from strangers were dropped. */
export function ignoredNote(n: number): string {
  if (n === 0) return t("channels.nessun_messaggio_ignorato");
  return n === 1 ? t("channels.1_messaggio_ignorato_da_sconosciuti") : t("channels.messaggi_ignorati_da_sconosciuti", { n });
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
  if (!a.host.trim()) return t("channels.scrivi_il_server_per_esempio");
  if (!a.username.trim()) return t("channels.scrivi_il_nome_utente");
  if (!Number.isInteger(a.port) || a.port < 1 || a.port > 65535) return t("channels.la_porta_e_un_numero");
  if (!a.password) return t("channels.scrivi_la_password_per_gmail");
  return null;
}

export const hasStoredPassword = (a: MailAccount): boolean => a.password === SECRET_MASK;

/** Folders typed one per line or separated by commas. */
export function parseFolders(raw: string): string[] {
  return raw.split(/[\n,]/).map((f) => f.trim()).filter(Boolean);
}
