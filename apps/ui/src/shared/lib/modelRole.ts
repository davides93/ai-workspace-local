import type { InferenceProfile, LocalModel } from "../api/types";
import { t } from "../i18n";

/** Role of a model in the simple interface, from what it is (never from a benchmark): a system model, or its size. */
export type ModelRoleId = "system" | "light" | "fast" | "smart";

/** Up to this many billion parameters: «light»; up to the next: «fast»; above it: «smart» (a mixture of experts counts its total). */
export const LIGHT_UP_TO_B = 3;
export const FAST_UP_TO_B = 9;

export function modelRoleId(model: Pick<LocalModel, "format" | "params_b"> | undefined): ModelRoleId | undefined {
  if (!model) return undefined;
  if (model.format === "system") return "system";
  const p = model.params_b;
  if (!p || p <= 0) return undefined;
  return p <= LIGHT_UP_TO_B ? "light" : p <= FAST_UP_TO_B ? "fast" : "smart";
}

export function modelRoleLabel(id: ModelRoleId): string {
  switch (id) {
    case "system": return t("lib.ruolo_integrato");
    case "light": return t("lib.ruolo_leggero");
    case "fast": return t("lib.ruolo_veloce");
    case "smart": return t("lib.ruolo_intelligente");
  }
}

/** The local model a profile uses, by its id. */
export function modelOfProfile(profile: Pick<InferenceProfile, "model"> | null | undefined, models: LocalModel[]): LocalModel | undefined {
  return profile?.model ? models.find((m) => m.id === profile.model) : undefined;
}

/** Label of the role of the model a profile uses; undefined when the model is not on disk or its size is not known. */
export function profileRoleLabel(profile: Pick<InferenceProfile, "model"> | null | undefined, models: LocalModel[]): string | undefined {
  const id = modelRoleId(modelOfProfile(profile, models));
  return id ? modelRoleLabel(id) : undefined;
}
