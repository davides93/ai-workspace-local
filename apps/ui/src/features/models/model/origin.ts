import type { ModelOrigin } from "../../../shared/api/types";

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
  aiw: { label: "AIW", owner: "AIW", description: "Scaricato da AIW nella cartella dei modelli.", external: false },
  manual: { label: "Manuale", owner: "te", description: "File copiato a mano nella cartella dei modelli di AIW.", external: false },
  ollama: { label: "Ollama", owner: "Ollama", description: "Letto in sola lettura dalla libreria di Ollama: non viene copiato.", external: true },
  lmstudio: { label: "LM Studio", owner: "LM Studio", description: "Letto in sola lettura dalla cartella di LM Studio: non viene copiato.", external: true },
  "hf-cache": { label: "Cache HF", owner: "la cache di Hugging Face", description: "Letto in sola lettura dalla cache di Hugging Face: non viene copiato.", external: true },
};

/** Display order of the origin filter. */
export const ORIGIN_ORDER: ModelOrigin[] = ["aiw", "manual", "ollama", "lmstudio", "hf-cache"];

/** Unknown or missing origin (older backends) is treated as a manual file. */
export function originInfo(origin: string | undefined): OriginInfo {
  return ORIGINS[(origin ?? "") as ModelOrigin] ?? ORIGINS.manual;
}

export const isExternalOrigin = (origin: string | undefined): boolean => originInfo(origin).external;
