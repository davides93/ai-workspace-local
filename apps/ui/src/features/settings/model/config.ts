import type { ConfigLayer, ConfigView, FieldSchema, SettingsSchema } from "../../../shared/api/types";
import type { Theme } from "../../../shared/lib/theme";
import { isUiMode, type UiMode } from "../../../shared/lib/uiMode";

export type LayerName = ConfigLayer["name"];
export type WritableLayer = Exclude<LayerName, "defaults">;

/** Layers you can write to, lowest priority first (a later one overrides an earlier one). */
export const WRITABLE: WritableLayer[] = ["global", "device", "workspace"];

export const LAYER_LABEL: Record<LayerName, string> = { defaults: "default", global: "globale", device: "device", workspace: "workspace" };

export const LAYER_HINT: Record<WritableLayer, string> = {
  global: "vale per tutti i workspace di questo utente",
  device: "vale solo su questo dispositivo",
  workspace: "vale solo per la cartella di lavoro aperta",
};

const layerOf = (view: ConfigView, name: LayerName) => view.layers.find((l) => l.name === name);

/** Value the layer itself defines for `key`, or `undefined` when it does not. */
export function ownValue(view: ConfigView, layer: WritableLayer, key: string): unknown {
  const values = layerOf(view, layer)?.values;
  return values && key in values ? values[key] : undefined;
}

export const hasOwn = (view: ConfigView, layer: WritableLayer, key: string) => ownValue(view, layer, key) !== undefined;

/** Writable layers that define `key`, lowest priority first. */
export function definingLayers(view: ConfigView, key: string): WritableLayer[] {
  return WRITABLE.filter((l) => hasOwn(view, l, key));
}

/** The layer whose value is the effective one: the highest-priority layer that defines the key, else the default. */
export function winningLayer(view: ConfigView, key: string): LayerName {
  const defining = definingLayers(view, key);
  return defining.length ? defining[defining.length - 1] : "defaults";
}

/** A layer above `layer` that defines the key, so a value written in `layer` would not show up (the first, by priority). */
export function overriddenBy(view: ConfigView, layer: WritableLayer, key: string): WritableLayer | null {
  const above = WRITABLE.slice(WRITABLE.indexOf(layer) + 1).filter((l) => hasOwn(view, l, key));
  return above.length ? above[above.length - 1] : null;
}

/** The value the effective configuration has for a setting. */
export const effectiveValue = (view: ConfigView, key: string, field: FieldSchema): unknown => view.effective[key] ?? field.default;

/** What the editor of `layer` shows: its own value, else the one it inherits. */
export function shownValue(view: ConfigView, layer: WritableLayer, key: string, field: FieldSchema): unknown {
  return hasOwn(view, layer, key) ? ownValue(view, layer, key) : effectiveValue(view, key, field);
}

export const sameValue = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

// ---------------------------------------------------------------- sections

export interface SettingsSection {
  id: string;
  title: string;
  description: string;
  prefixes: string[];
}

export const SECTIONS: SettingsSection[] = [
  { id: "ui", title: "Interfaccia", description: "Modalità, tema e lingua.", prefixes: ["ui"] },
  { id: "app", title: "Programma", description: "Cosa fa l'app quando chiudi la finestra, all'accesso al computer e per gli avvisi (solo nell'app per computer).", prefixes: ["app"] },
  { id: "agents", title: "Agenti", description: "Agente predefinito, subagenti, budget dei run e gestione del contesto.", prefixes: ["agent", "agents", "context"] },
  { id: "security", title: "Sicurezza e rete", description: "Conferme, sandbox e siti raggiungibili da web e browser.", prefixes: ["policy", "sandbox", "network"] },
  { id: "server", title: "Server e accesso", description: "Come altri dispositivi e programmi raggiungono il workspace.", prefixes: ["server"] },
  { id: "folders", title: "Cartelle", description: "Dove stanno i modelli e dove lavorano gli agenti.", prefixes: ["models", "workspace"] },
  { id: "memory", title: "Memoria", description: "Ciò che l'assistente ricorda di te resta su questo dispositivo; lo gestisci dalla sezione Insegna.", prefixes: ["memory"] },
  { id: "learning", title: "Apprendimento", description: "Lezioni che gli agenti propongono dopo una conversazione e ottimizzazione del prompt: niente cambia senza la tua approvazione.", prefixes: ["learning"] },
  { id: "proactive", title: "Assistente proattivo", description: "Spento finché non lo accendi. Controlla ogni tanto cosa c'è di nuovo e ti lascia delle proposte nella sezione Proposte: non fa mai nulla da solo.", prefixes: ["proactive"] },
  { id: "connectors", title: "Calendario e posta", description: "Sola lettura, e i dati restano su questo dispositivo: non scrive, non invia, non cancella.", prefixes: ["connectors"] },
  { id: "search", title: "Ricerca web", description: "Il testo cercato viene inviato al motore scelto. SearXNG può girare su questo computer, così le ricerche non escono dalla tua rete.", prefixes: ["search"] },
  { id: "voice", title: "Voce", description: "Dettatura e conversazione a voce: lingua, motore di trascrizione e voce che legge le risposte.", prefixes: ["voice"] },
  { id: "update", title: "Canale e aggiornamenti", description: "Quale canale seguire (produzione, beta, alpha) e se cercare e scaricare gli aggiornamenti da soli.", prefixes: ["update"] },
  { id: "telemetry", title: "Telemetria", description: "Esportazione di tracce e metriche.", prefixes: ["telemetry"] },
];

const OTHER: SettingsSection = { id: "other", title: "Altro", description: "", prefixes: [] };

export function sectionOf(key: string): SettingsSection {
  const prefix = key.split(".")[0];
  return SECTIONS.find((s) => s.prefixes.includes(prefix)) ?? OTHER;
}

/** Settings only the developer view shows: they are plumbing for the person who set the assistant up, not choices of an everyday user. */
export const DEVELOPER_ONLY: readonly string[] = ["proactive.watch_folders", "proactive.agent"];

export interface SettingEntry {
  key: string;
  field: FieldSchema;
}

/** Help line of a setting: the schema's own description, else a note written here. */
const FIELD_HINT: Record<string, string> = {
  "ui.theme": "È la stessa scelta del selettore in alto: cambiando una cambia l'altra.",
  "ui.language": "Per ora l'interfaccia è disponibile solo in italiano.",
  "network.allow_domains": "Un dominio per etichetta (es. docs.rs). Vuoto vale per tutti i siti pubblici. Riguarda web_fetch e il browser controllato.",
  "server.lan": "Rende il server raggiungibile dagli altri dispositivi della rete; da fuori serve sempre il token di pairing.",
  "server.port": "Porta del server API.",
  "models.dir": "Cartella in cui si scaricano i modelli. Vuoto usa quella predefinita.",
  "workspace.root": "Cartella su cui lavorano gli agenti. Cambiandola si sposta anche il file dello strato workspace.",
  "telemetry.otlp": "Indirizzo OTLP (per esempio http://localhost:4318). Vuoto lo disattiva.",
  "agents.max_parallel_subagents": "Quanti subagenti possono lavorare insieme quando l'agente ne delega più di uno nello stesso turno.",
  "agents.budget.max_tokens": "Limite per tutti gli agenti che non ne hanno uno proprio. 0 = nessun limite.",
  "agents.budget.max_turns": "Limite per tutti gli agenti che non ne hanno uno proprio. 0 = nessun limite.",
  "agents.budget.max_seconds": "Limite per tutti gli agenti che non ne hanno uno proprio. 0 = nessun limite.",
  "context.compact_at": "Quota di contesto occupato oltre la quale la conversazione viene compattata.",
};

/** Title without its trailing remark in brackets ("Domini consentiti (vuoto = tutti)" → "Domini consentiti"), for toasts. */
export const shortTitle = (title: string) => title.replace(/\s*\([^)]*\)\s*$/, "").trim() || title;

export const hintOf = (key: string, field: FieldSchema) => field.description ?? FIELD_HINT[key];

function matches(e: SettingEntry, q: string): boolean {
  if (!q) return true;
  return [e.key, e.field.title ?? "", hintOf(e.key, e.field) ?? ""].some((s) => s.toLowerCase().includes(q));
}

/** Settings grouped by section in the fixed order; `query` filters, sections left empty are dropped. */
export function groupSettings(schema: SettingsSchema, query = ""): { section: SettingsSection; entries: SettingEntry[] }[] {
  const q = query.trim().toLowerCase();
  const all: SettingEntry[] = Object.entries(schema.properties).map(([key, field]) => ({ key, field }));
  return [...SECTIONS, OTHER]
    .map((section) => ({ section, entries: all.filter((e) => sectionOf(e.key).id === section.id && matches(e, q)) }))
    .filter((g) => g.entries.length > 0);
}

// ---------------------------------------------------------------- values

export type Parsed = { ok: true; value: number } | { ok: false; error: string };

/** Reads a number field: whole numbers for `integer`, inside `minimum`..`maximum` when the schema has them. */
export function parseNumber(raw: string, field: Pick<FieldSchema, "type" | "minimum" | "maximum">): Parsed {
  const text = raw.trim().replace(",", ".");
  if (text === "") return { ok: false, error: "Inserisci un numero" };
  const n = Number(text);
  if (!Number.isFinite(n)) return { ok: false, error: "Non è un numero" };
  if (field.type === "integer" && !Number.isInteger(n)) return { ok: false, error: "Serve un numero intero" };
  if (field.minimum !== undefined && n < field.minimum) return { ok: false, error: `Il minimo è ${field.minimum}` };
  if (field.maximum !== undefined && n > field.maximum) return { ok: false, error: `Il massimo è ${field.maximum}` };
  return { ok: true, value: n };
}

/** A value as the read-only summary shows it. */
export function formatValue(v: unknown): string {
  if (v === undefined || v === null || v === "") return "vuoto";
  if (Array.isArray(v)) return v.length ? v.join(", ") : "vuoto";
  if (typeof v === "boolean") return v ? "sì" : "no";
  return String(v);
}

/** Readable names of the choices of enum settings; a value without a name is shown as it is. */
const OPTION_LABEL: Record<string, Record<string, string>> = {
  "ui.mode": { simple: "Semplice", developer: "Sviluppatore" },
  "ui.theme": { system: "Sistema", light: "Chiaro", dark: "Scuro" },
  "proactive.level": { off: "Spento", propose: "Propone", act_readonly: "Propone e legge (anche il web, sola lettura)" },
  "ui.language": { it: "Italiano", en: "English" },
  "search.provider": { auto: "Automatico", duckduckgo: "DuckDuckGo", searxng: "SearXNG", brave: "Brave Search", off: "Spenta" },
  "policy.mode": {
    plan: "plan · solo lettura",
    ask: "ask · chiede per modifiche e comandi",
    "auto-edit": "auto-edit · modifica i file, chiede per i comandi",
    auto: "auto · chiede solo per computer-use",
    yolo: "yolo · non chiede mai",
  },
  "voice.stt_runtime": {
    auto: "auto · whisper.cpp, poi un server locale, poi un dispositivo accoppiato",
    "whisper-server": "whisper.cpp",
    localai: "LocalAI",
    nexa: "Nexa",
    "aiw-peer": "Dispositivo accoppiato",
    "remote-openai": "Endpoint compatibile OpenAI",
  },
  "learning.reflect": { off: "off · mai", on_feedback: "on_feedback · dopo un pollice giù", on_failure: "on_failure · anche dopo un run fallito o fermato dal budget" },
  "sandbox.mode": { "read-only": "read-only · sola lettura", "workspace-write": "workspace-write · scrive nel workspace", off: "off · nessuna sandbox" },
};

export const optionLabel = (key: string, value: string | number): string => OPTION_LABEL[key]?.[String(value)] ?? String(value);

export const THEMES: Theme[] = ["system", "light", "dark"];
export const THEME_LABEL: Record<Theme, string> = { system: "Sistema", light: "Chiaro", dark: "Scuro" };
export const isTheme = (v: unknown): v is Theme => typeof v === "string" && (THEMES as string[]).includes(v);

/** The theme the configuration asks for, when it is a valid one. */
export function configTheme(view: ConfigView | undefined): Theme | undefined {
  const v = view?.effective["ui.theme"];
  return isTheme(v) ? v : undefined;
}

/** The interface mode the configuration asks for, when it is a valid one. */
export function configUiMode(view: ConfigView | undefined): UiMode | undefined {
  const v = view?.effective["ui.mode"];
  return isUiMode(v) ? v : undefined;
}

/**
 * Layer a mode switch is written to: the one that wins today (so the change is visible), else the
 * global one, like the other preferences of the person rather than of a device or folder.
 */
export function uiModeLayer(view: ConfigView | undefined): WritableLayer {
  if (!view) return "global";
  const w = winningLayer(view, "ui.mode");
  return w === "defaults" ? "global" : w;
}
