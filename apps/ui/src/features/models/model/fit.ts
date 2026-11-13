import type { Fit } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";

import { t } from "../../../shared/i18n";
/** How well a model fits this device, as label and pill tone. */
export const FIT: Record<Fit, { label: string; tone: PillTone }> = {
  great: { get label() { return t("models.ci_sta_comodo"); }, tone: "ok" },
  ok: { get label() { return t("models.ci_sta"); }, tone: "info" },
  tight: { get label() { return t("models.al_limite"); }, tone: "warn" },
  no: { get label() { return t("models.non_ci_sta"); }, tone: "crit" },
};

/** Formats a provider can supply; the Scopri filter uses the provider's own list when it has one. */
export const PROVIDER_FORMATS = ["gguf", "mlx", "onnx", "safetensors", "litertlm"] as const;

/** 0 = best. Unknown or empty fit (older backends) sorts last. */
export function fitRank(fit: string | undefined): number {
  switch (fit) {
    case "great": return 0;
    case "ok": return 1;
    case "tight": return 2;
    case "no": return 3;
    default: return 4;
  }
}

export function isFit(fit: string | undefined): fit is Fit {
  return fit === "great" || fit === "ok" || fit === "tight" || fit === "no";
}
