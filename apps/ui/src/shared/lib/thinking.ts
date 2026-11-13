import type { LocalModel, ThinkingMode } from "../api/types";

import { t } from "../i18n";
/** The "Ragionamento" choices, in the order they are shown. */
export const THINKING_MODES: ThinkingMode[] = ["off", "auto", "deep"];

export const THINKING_LABEL: Record<ThinkingMode, string> = { get off() { return t("lib.spento"); }, get auto() { return t("lib.automatico"); }, get deep() { return t("lib.approfondito"); } };

/** One sentence per choice, for the title of the control. */
export const THINKING_HINT: Record<ThinkingMode, string> = {
  get off() { return t("lib.risponde_subito_senza_pensarci_su"); },
  get auto() { return t("lib.ragiona_all_inizio_e_quando"); },
  get deep() { return t("lib.ragiona_sempre_e_piu_a"); },
};

export const isThinkingMode = (v: unknown): v is ThinkingMode => v === "off" || v === "auto" || v === "deep";

/** The model is marked (catalog and scan) as one that can reason; only then is the setting offered. */
export const canReason = (model: Pick<LocalModel, "capabilities"> | undefined | null): boolean => !!model?.capabilities?.includes("reasoning");

/** What the chat sends for a run: the choice made in this session, else the profile's, else automatic. */
export const effectiveThinking = (chosen: ThinkingMode | undefined, profile: ThinkingMode | undefined): ThinkingMode => chosen ?? profile ?? "auto";
