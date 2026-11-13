import type { InferenceParams } from "../../../shared/api/types";

import { t } from "../../../shared/i18n";
/** "Stile di risposta": the three presets that stand for temperature and top-p. */
export type StyleId = "precise" | "balanced" | "creative";

export interface StylePreset {
  id: StyleId;
  label: string;
  hint: string;
  temperature: number;
  top_p: number;
}

export const STYLE_PRESETS: StylePreset[] = [
  { id: "precise", get label() { return t("agents.preciso"); }, get hint() { return t("agents.risposte_sobrie_e_prevedibili"); }, temperature: 0.2, top_p: 0.9 },
  { id: "balanced", get label() { return t("agents.bilanciato"); }, get hint() { return t("agents.un_buon_equilibrio_tra_precisione"); }, temperature: 0.6, top_p: 0.95 },
  { id: "creative", get label() { return t("agents.creativo"); }, get hint() { return t("agents.piu_varieta_e_idee_insolite"); }, temperature: 0.9, top_p: 0.95 },
];

/** What the style control shows: a preset, the model's own defaults, or numbers set by hand (developer view). */
export type StyleChoice = StyleId | "default" | "custom";

/** The preset that `params` stand for. Both numbers must match: any other mix is `custom`, none is `default`. */
export function styleOf(params: Pick<InferenceParams, "temperature" | "top_p">): StyleChoice {
  const { temperature, top_p } = params;
  if (temperature === undefined && top_p === undefined) return "default";
  return STYLE_PRESETS.find((p) => p.temperature === temperature && p.top_p === top_p)?.id ?? "custom";
}

/**
 * `params` with the style applied; every other value (context, max tokens, adapters, reasoning…) is
 * kept as it is. `default` forgets the two numbers; `custom` is not something to apply.
 */
export function withStyle<P extends InferenceParams>(params: P, choice: StyleChoice): P {
  if (choice === "custom") return params;
  const { temperature: _t, top_p: _p, ...rest } = params;
  const preset = STYLE_PRESETS.find((p) => p.id === choice);
  return (preset ? { ...rest, temperature: preset.temperature, top_p: preset.top_p } : rest) as P;
}
