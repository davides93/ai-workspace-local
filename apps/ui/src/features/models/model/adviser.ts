import type { AdviserTask, CatalogModel, InferenceProfile, ModelChoice, Recommendations } from "../../../shared/api/types";
import type { UiMode } from "../../../shared/lib/uiMode";

// The model adviser on the UI side: `auto:<task>` inference profiles, what a picker offers for
// them, and the "only what runs here" filter of the simple Modelli. Pure, tested without a DOM.

/** Tasks an agent's inference profile can follow (`embeddings` is not an agent profile). */
export type AutoTask = Exclude<AdviserTask, "embeddings">;

export const AUTO_PREFIX = "auto:";
/** What "Automatico (consigliato)" maps to: reliable tool calling with room for the conversation. */
export const AUTO_RECOMMENDED = "auto:agent";

const AUTO_LABEL: Record<AutoTask, string> = {
  agent: "Automatico (consigliato)",
  chat: "Automatico: chat veloce",
  reasoning: "Automatico: ragionamento",
  vision: "Automatico: immagini",
};

/** The task of an `auto:<task>` profile id, or undefined for any other id. */
export function autoTask(id: string | undefined): AutoTask | undefined {
  if (!id?.startsWith(AUTO_PREFIX)) return undefined;
  const task = id.slice(AUTO_PREFIX.length);
  return task in AUTO_LABEL ? (task as AutoTask) : undefined;
}

export const isAutoProfile = (id: string | undefined): boolean => autoTask(id) !== undefined;

export interface ProfileChoice {
  value: string;
  label: string;
}

/**
 * The automatic entries of a profile picker, first in the list. Simple mode offers only
 * "Automatico (consigliato)" (plus the one an agent already has); the developer view all four.
 */
export function autoChoices(mode: UiMode, current?: string): ProfileChoice[] {
  const tasks = (Object.keys(AUTO_LABEL) as AutoTask[]).filter((t) => mode === "developer" || t === "agent" || `${AUTO_PREFIX}${t}` === current);
  return tasks.map((t) => ({ value: `${AUTO_PREFIX}${t}`, label: AUTO_LABEL[t] }));
}

/** What an automatic profile points at right now ("Ora usa Qwen3 8B"), or why it cannot yet. */
export function autoSummary(id: string, rec: Recommendations | null | undefined): string | undefined {
  const task = autoTask(id);
  if (!task) return undefined;
  if (!rec) return "Sceglie da solo il modello più adatto tra quelli installati.";
  const choice = rec[task].installed;
  if (choice) return `Ora usa ${choice.name}. ${choice.reason}`;
  const dl = rec[task].download;
  return dl ? `Nessun modello adatto è installato: scarica ${dl.name} da Modelli.` : "Nessun modello installato è adatto a questo compito.";
}

/** The profile an `auto:<task>` id stands for right now (what the engine resolves at run start), for screens that show a model. */
export function autoProfileFromAdvice(id: string, rec: Recommendations | null | undefined): InferenceProfile | null {
  const task = autoTask(id);
  const c = task ? rec?.[task].installed : null;
  if (!task || !c) return null;
  return { id, name: AUTO_LABEL[task], runtime_id: c.runtime_id, model: c.model, params: { context: c.context, ...(c.thinking ? { thinking: c.thinking } : {}) } };
}

/** The catalog entry behind a download proposal (by catalog id), when the catalog has it. */
export function catalogForChoice(choice: ModelChoice | null | undefined, catalog: CatalogModel[]): CatalogModel | undefined {
  const id = choice?.catalog_id;
  return id ? catalog.find((c) => c.id === id && c.downloadable !== false && !c.installed) : undefined;
}

/** In simple mode only what can run on this device, unless the person asks for everything. */
export function runnableOnly<T extends { fit: string }>(list: T[], showAll: boolean): T[] {
  return showAll ? list : list.filter((m) => m.fit !== "no");
}

/** How many entries "Mostra tutti" would add. */
export const hiddenCount = (list: { fit: string }[]): number => list.filter((m) => m.fit === "no").length;
