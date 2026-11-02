import type { Fit } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";

/** How well a model fits this device, as label and pill tone. */
export const FIT: Record<Fit, { label: string; tone: PillTone }> = {
  great: { label: "Ci sta comodo", tone: "ok" },
  ok: { label: "Ci sta", tone: "info" },
  tight: { label: "Al limite", tone: "warn" },
  no: { label: "Non ci sta", tone: "crit" },
};

export const PROVIDER_FORMATS = ["gguf", "mlx", "onnx", "safetensors", "litertlm"] as const;
