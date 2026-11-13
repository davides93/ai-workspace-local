import type { ConfigLayer, ConfigView, FieldSchema, SettingsSchema } from "../../../shared/api/types";
import { labelMap, t } from "../../../shared/i18n";
import type { Theme } from "../../../shared/lib/theme";
import { isUiMode, type UiMode } from "../../../shared/lib/uiMode";

export type LayerName = ConfigLayer["name"];
export type WritableLayer = Exclude<LayerName, "defaults">;

/** Layers you can write to, lowest priority first (a later one overrides an earlier one). */
export const WRITABLE: WritableLayer[] = ["global", "device", "workspace"];

export const LAYER_LABEL: Record<LayerName, string> = { defaults: "default", global: "globale", device: "device", workspace: "workspace" };

export const LAYER_HINT: Record<WritableLayer, string> = {
  get global() { return t("settings.vale_per_tutti_i_workspace"); },
  get device() { return t("settings.vale_solo_su_questo_dispositivo"); },
  get workspace() { return t("settings.vale_solo_per_la_cartella"); },
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
  { id: "ui", get title() { return t("settings.interfaccia"); }, get description() { return t("settings.modalita_tema_e_lingua"); }, prefixes: ["ui"] },
  { id: "app", get title() { return t("settings.programma"); }, get description() { return t("settings.cosa_fa_l_app_quando"); }, prefixes: ["app"] },
  { id: "agents", get title() { return t("settings.agenti"); }, get description() { return t("settings.agente_predefinito_subagenti_budget_dei"); }, prefixes: ["agent", "agents", "context"] },
  { id: "security", get title() { return t("settings.sicurezza_e_rete"); }, get description() { return t("settings.conferme_sandbox_e_siti_raggiungibili"); }, prefixes: ["policy", "sandbox", "network"] },
  { id: "server", get title() { return t("settings.server_e_accesso"); }, get description() { return t("settings.come_altri_dispositivi_e_programmi"); }, prefixes: ["server"] },
  { id: "folders", get title() { return t("settings.cartelle"); }, get description() { return t("settings.dove_stanno_i_modelli_e"); }, prefixes: ["models", "workspace"] },
  { id: "memory", get title() { return t("settings.memoria"); }, get description() { return t("settings.cio_che_l_assistente_ricorda"); }, prefixes: ["memory"] },
  { id: "learning", get title() { return t("settings.apprendimento"); }, get description() { return t("settings.lezioni_che_gli_agenti_propongono"); }, prefixes: ["learning"] },
  { id: "proactive", get title() { return t("settings.assistente_proattivo"); }, get description() { return t("settings.spento_finche_non_lo_accendi"); }, prefixes: ["proactive"] },
  { id: "connectors", get title() { return t("settings.calendario_e_posta"); }, get description() { return t("settings.sola_lettura_e_i_dati"); }, prefixes: ["connectors"] },
  { id: "search", get title() { return t("settings.ricerca_web"); }, get description() { return t("settings.il_testo_cercato_viene_inviato"); }, prefixes: ["search"] },
  { id: "voice", get title() { return t("settings.voce"); }, get description() { return t("settings.dettatura_e_conversazione_a_voce"); }, prefixes: ["voice"] },
  { id: "update", get title() { return t("settings.canale_e_aggiornamenti"); }, get description() { return t("settings.quale_canale_seguire_produzione_beta"); }, prefixes: ["update"] },
  { id: "telemetry", get title() { return t("settings.telemetria"); }, get description() { return t("settings.esportazione_di_tracce_e_metriche"); }, prefixes: ["telemetry"] },
];

const OTHER: SettingsSection = { id: "other", get title() { return t("settings.altro"); }, description: "", prefixes: [] };

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
  get "ui.theme"() { return t("settings.e_la_stessa_scelta_del"); },
  get "network.allow_domains"() { return t("settings.un_dominio_per_etichetta_es"); },
  get "server.lan"() { return t("settings.rende_il_server_raggiungibile_dagli"); },
  get "server.port"() { return t("settings.porta_del_server_api"); },
  get "models.dir"() { return t("settings.cartella_in_cui_si_scaricano"); },
  get "workspace.root"() { return t("settings.cartella_su_cui_lavorano_gli"); },
  get "telemetry.otlp"() { return t("settings.indirizzo_otlp_per_esempio_http"); },
  get "agents.max_parallel_subagents"() { return t("settings.quanti_subagenti_possono_lavorare_insieme"); },
  get "agents.budget.max_tokens"() { return t("settings.limite_per_tutti_gli_agenti"); },
  get "agents.budget.max_turns"() { return t("settings.limite_per_tutti_gli_agenti"); },
  get "agents.budget.max_seconds"() { return t("settings.limite_per_tutti_gli_agenti"); },
  get "context.compact_at"() { return t("settings.quota_di_contesto_occupato_oltre"); },
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
  if (text === "") return { ok: false, error: t("settings.inserisci_un_numero") };
  const n = Number(text);
  if (!Number.isFinite(n)) return { ok: false, error: t("settings.non_e_un_numero") };
  if (field.type === "integer" && !Number.isInteger(n)) return { ok: false, error: t("settings.serve_un_numero_intero") };
  if (field.minimum !== undefined && n < field.minimum) return { ok: false, error: t("settings.il_minimo_e", { minimum: field.minimum }) };
  if (field.maximum !== undefined && n > field.maximum) return { ok: false, error: t("settings.il_massimo_e", { maximum: field.maximum }) };
  return { ok: true, value: n };
}

/** A value as the read-only summary shows it. */
export function formatValue(v: unknown): string {
  if (v === undefined || v === null || v === "") return t("settings.vuoto");
  if (Array.isArray(v)) return v.length ? v.join(", ") : t("settings.vuoto");
  if (typeof v === "boolean") return v ? t("settings.si") : t("settings.no");
  return String(v);
}

/** Readable names of the choices of enum settings; a value without a name is shown as it is. */
const OPTION_LABEL: Record<string, Record<string, string>> = {
  "ui.mode": { get simple() { return t("settings.semplice"); }, get developer() { return t("settings.sviluppatore"); } },
  "ui.theme": { get system() { return t("settings.sistema"); }, get light() { return t("settings.chiaro"); }, get dark() { return t("settings.scuro"); } },
  "proactive.level": { get off() { return t("settings.spento"); }, get propose() { return t("settings.propone"); }, get act_readonly() { return t("settings.propone_e_legge_anche_il"); } },
  "ui.language": { get auto() { return t("settings.automatica"); }, it: "Italiano", en: "English" },
  "search.provider": { get auto() { return t("settings.automatico"); }, duckduckgo: "DuckDuckGo", searxng: "SearXNG", get brave() { return t("settings.brave_search"); }, get off() { return t("settings.spenta"); } },
  "policy.mode": {
    get plan() { return t("settings.plan_solo_lettura"); },
    get ask() { return t("settings.ask_chiede_per_modifiche_e"); },
    get "auto-edit"() { return t("settings.auto_edit_modifica_i_file"); },
    get auto() { return t("settings.auto_chiede_solo_per_computer"); },
    get yolo() { return t("settings.yolo_non_chiede_mai"); },
  },
  "voice.stt_runtime": {
    get auto() { return t("settings.auto_whisper_cpp_poi_un"); },
    "whisper-server": "whisper.cpp",
    localai: "LocalAI",
    get nexa() { return t("settings.nexa"); },
    get "aiw-peer"() { return t("settings.dispositivo_accoppiato"); },
    get "remote-openai"() { return t("settings.endpoint_compatibile_openai"); },
  },
  "learning.reflect": { get off() { return t("settings.off_mai"); }, get on_feedback() { return t("settings.on_feedback_dopo_un_pollice"); }, get on_failure() { return t("settings.on_failure_anche_dopo_un"); } },
  "sandbox.mode": { get "read-only"() { return t("settings.read_only_sola_lettura"); }, get "workspace-write"() { return t("settings.workspace_write_scrive_nel_workspace"); }, get off() { return t("settings.off_nessuna_sandbox"); } },
};

export const optionLabel = (key: string, value: string | number): string => OPTION_LABEL[key]?.[String(value)] ?? String(value);

export const THEMES: Theme[] = ["system", "light", "dark"];
export const THEME_LABEL: Record<Theme, string> = labelMap({ system: "shell.sistema", light: "shell.chiaro", dark: "shell.scuro" });
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
