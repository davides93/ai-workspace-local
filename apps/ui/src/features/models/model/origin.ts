import type { ModelOrigin } from "../../../shared/api/types";

import { t } from "../../../shared/i18n";
export interface OriginInfo {
  label: string;
  /** Name of the app that owns the files, for the explanations ("gestito da Ollama"). */
  owner: string;
  description: string;
  /** Files live in another app's store: AIW reads them in place, never copies or deletes them. */
  external: boolean;
}

/** Where an installed model comes from (docs/architecture/model-hub.md §6 and §9). */
export const ORIGINS: Record<ModelOrigin, OriginInfo> = {
  system: { get label() { return t("models.di_sistema"); }, get owner() { return t("models.il_sistema_operativo"); }, get description() { return t("models.fornito_dal_dispositivo_senza_download"); }, external: false },
  aiw: { get label() { return t("models.aiw"); }, owner: "AIW", get description() { return t("models.scaricato_da_aiw_nella_cartella"); }, external: false },
  manual: { get label() { return t("models.manuale"); }, owner: "te", get description() { return t("models.file_copiato_a_mano_nella"); }, external: false },
  ollama: { get label() { return t("models.ollama"); }, get owner() { return t("models.ollama"); }, get description() { return t("models.letto_in_sola_lettura_dalla"); }, external: true },
  lmstudio: { get label() { return t("models.lm_studio"); }, owner: "LM Studio", get description() { return t("models.letto_in_sola_lettura_dalla_2"); }, external: true },
  "hf-cache": { get label() { return t("models.cache_hf"); }, get owner() { return t("models.la_cache_di_hugging_face"); }, get description() { return t("models.letto_in_sola_lettura_dalla_3"); }, external: true },
};

/** Display order of the origin filter. */
export const ORIGIN_ORDER: ModelOrigin[] = ["system", "aiw", "manual", "ollama", "lmstudio", "hf-cache"];

/** Unknown or missing origin (older backends) is treated as a manual file. */
export function originInfo(origin: string | undefined): OriginInfo {
  return ORIGINS[(origin ?? "") as ModelOrigin] ?? ORIGINS.manual;
}

export const isExternalOrigin = (origin: string | undefined): boolean => originInfo(origin).external;
