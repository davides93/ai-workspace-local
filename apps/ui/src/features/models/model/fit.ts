import type { Fit } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";

/** How well a model fits this device, as label and pill tone. */
export const FIT: Record<Fit, { label: string; tone: PillTone }> = {
  great: { label: "Ci sta comodo", tone: "ok" },
  ok: { label: "Ci sta", tone: "info" },
  tight: { label: "Al limite", tone: "warn" },
  no: { label: "Non ci sta", tone: "crit" },
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
