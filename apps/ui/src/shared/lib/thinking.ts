import type { LocalModel, ThinkingMode } from "../api/types";

/** The "Ragionamento" choices, in the order they are shown. */
export const THINKING_MODES: ThinkingMode[] = ["off", "auto", "deep"];

export const THINKING_LABEL: Record<ThinkingMode, string> = { off: "Spento", auto: "Automatico", deep: "Approfondito" };

/** One sentence per choice, for the title of the control. */
export const THINKING_HINT: Record<ThinkingMode, string> = {
  off: "Risponde subito, senza pensarci su.",
  auto: "Ragiona all'inizio e quando qualcosa non va, poi procede veloce.",
  deep: "Ragiona sempre e più a lungo: più lento, più accurato.",
};

export const isThinkingMode = (v: unknown): v is ThinkingMode => v === "off" || v === "auto" || v === "deep";

/** The model is marked (catalog and scan) as one that can reason; only then is the setting offered. */
export const canReason = (model: Pick<LocalModel, "capabilities"> | undefined | null): boolean => !!model?.capabilities?.includes("reasoning");

/** What the chat sends for a run: the choice made in this session, else the profile's, else automatic. */
export const effectiveThinking = (chosen: ThinkingMode | undefined, profile: ThinkingMode | undefined): ThinkingMode => chosen ?? profile ?? "auto";
