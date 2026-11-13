import type { ModelProviderInfo, RemoteModel } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";
import { PROVIDER_FORMATS } from "./fit";

import { t as tr } from "../../../shared/i18n";
/** The curated catalog has its own tab, so it is not offered as a discovery source. */
export function discoverProviders(all: ModelProviderInfo[]): ModelProviderInfo[] {
  return all.filter((p) => p.id !== "catalog");
}

/** Provider shown first in Scopri, and the one used when the URL names an unknown provider. */
export const FALLBACK_PROVIDER = "hf";

const PROVIDER_NAMES: Record<string, string> = {
  catalog: "Consigliati",
  hf: "Hugging Face",
  modelscope: "ModelScope",
  ollama: "Registry Ollama",
  url: "URL diretto",
};

/** Display name of a provider id (the backend's own name wins when the list is loaded). */
export function providerName(id: string, list?: Pick<ModelProviderInfo, "id" | "name">[]): string {
  return list?.find((p) => p.id === id)?.name ?? PROVIDER_NAMES[id] ?? id;
}

/** The provider the URL asks for when it is one of the offered ones, else the first offered (Hugging Face when known). */
export function pickProvider(offered: Pick<ModelProviderInfo, "id">[], requested: string | undefined): string {
  if (requested && offered.some((p) => p.id === requested)) return requested;
  return offered.some((p) => p.id === FALLBACK_PROVIDER) ? FALLBACK_PROVIDER : (offered[0]?.id ?? FALLBACK_PROVIDER);
}

export function isUrlProvider(id: string): boolean {
  return id === "url";
}

/**
 * What a provider's own description does not say: how to reach a model its search cannot list.
 * Hugging Face and the direct URL need no extra note (their description and the form say it).
 */
export function providerNote(id: string): string | null {
  switch (id) {
    case "ollama":
      return "Ollama non ha una ricerca pubblica: qui cerchi in un indice locale dei modelli più popolari. Per gli altri scrivi il nome esatto con il tag, per esempio qwen3:4b oppure utente/nome:tag, e aprilo da lì.";
    case "modelscope":
      return "La ricerca usa un'API del sito non documentata (best effort). Se un modello non compare, scrivi il nome esatto nella forma proprietario/nome e aprilo da lì.";
    default:
      return null;
  }
}

export interface AuthBadge {
  label: string;
  tone: PillTone;
  /** Longer explanation for the tooltip and the settings dialog. */
  detail: string;
}

/** Token state of a provider as a badge: set, optional, required or not used. */
export function authBadge(p: Pick<ModelProviderInfo, "auth" | "auth_configured">): AuthBadge {
  if (p.auth === "none") return { label: tr("models.nessun_token"), tone: "neutral", detail: tr("models.questo_provider_non_richiede_un") };
  if (p.auth_configured) return { label: tr("models.token_impostato"), tone: "ok", detail: tr("models.un_token_e_salvato_su") };
  if (p.auth === "required") return { label: tr("models.token_richiesto"), tone: "warn", detail: tr("models.senza_token_questo_provider_non") };
  return { label: tr("models.token_opzionale"), tone: "neutral", detail: tr("models.funziona_senza_token_serve_per") };
}

/** Token and/or endpoint can be changed from the settings dialog. */
export function hasSettings(p: Pick<ModelProviderInfo, "auth" | "endpoint">): boolean {
  return p.auth !== "none" || p.endpoint !== "";
}

/** Formats offered by the filter of a provider: its own list when known, else the common ones. */
export function formatOptions(p: Pick<ModelProviderInfo, "formats"> | undefined): string[] {
  const own = (p?.formats ?? []).filter((f) => f !== "system");
  return own.length > 0 ? own : [...PROVIDER_FORMATS];
}

/**
 * Exact name typed in the search box, when it looks like a model reference of the provider:
 * `owner/name` for Hugging Face and ModelScope, `name[:tag]` or `user/name[:tag]` for Ollama.
 * Used to open a model that the (index or best-effort) search does not list.
 */
export function exactRef(provider: string, q: string): string | null {
  const t = q.trim();
  if (!t) return null;
  if (provider === "hf" || provider === "modelscope") return /^[\w.-]+\/[\w.-]+$/.test(t) ? t : null;
  if (provider === "ollama") return /^([\w.-]+\/)?[\w.-]+(:[\w.-]+)?$/.test(t) ? t : null;
  return null;
}

/** Revision to pin a download to: meaningful (commit or branch) for the repository hubs, not for Ollama's tags. */
export function pullRevision(provider: string, revision: string | undefined): string | undefined {
  return (provider === "hf" || provider === "modelscope") && revision ? revision : undefined;
}

/** A minimal model record for a reference that is not in the search results; the detail call fills in the rest. */
export function stubModel(provider: string, id: string): RemoteModel {
  const slash = id.indexOf("/");
  const name = slash >= 0 ? id.slice(slash + 1) : id;
  return {
    provider,
    id,
    name,
    publisher: slash >= 0 ? id.slice(0, slash) : provider === "ollama" ? "library" : "",
    formats: [],
    tags: [],
    capabilities: [],
    gated: false,
    url: "",
  };
}

/** Label of a sort key in the Scopri filter. */
export const SORT_OPTIONS = [
  { id: "downloads", label: "Più scaricati" },
  { id: "likes", label: "Più apprezzati" },
  { id: "updated", label: "Aggiornati" },
] as const;

export type SortKey = (typeof SORT_OPTIONS)[number]["id"];

/** Ollama's registry has no likes, so sorting by them is not offered there. */
export function sortOptionsFor(provider: string): { id: SortKey; label: string }[] {
  return SORT_OPTIONS.filter((o) => !(provider === "ollama" && o.id === "likes")).map((o) => ({ id: o.id, label: o.label }));
}

export function parseSort(v: string | undefined): SortKey {
  return SORT_OPTIONS.some((o) => o.id === v) ? (v as SortKey) : "downloads";
}
