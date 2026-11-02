import type { InferenceProfile, LocalModel, RuntimeCompat } from "../../../shared/api/types";

/** Profiles that run `modelId` (they stop working if the model is deleted). */
export function profilesUsingModel(profiles: InferenceProfile[], modelId: string): InferenceProfile[] {
  return profiles.filter((p) => p.model === modelId);
}

/** Runtimes that can run the model, the ones available on this device first (keeps the backend order inside each group). */
export function runtimeOptions(model: Pick<LocalModel, "runtime_compat">): RuntimeCompat[] {
  const all = model.runtime_compat ?? [];
  return [...all.filter((r) => r.available), ...all.filter((r) => !r.available)];
}

/**
 * Runtime to preselect for a model: `preferred` (the one the profile already uses) when it can run the
 * model on this device, else the first available one, else null (nothing can run it here).
 */
export function pickRuntime(model: Pick<LocalModel, "runtime_compat">, preferred?: string): string | null {
  const usable = runtimeOptions(model).filter((r) => r.available);
  if (preferred && usable.some((r) => r.id === preferred)) return preferred;
  return usable[0]?.id ?? null;
}

/** Lowercase ASCII slug of a name ("Qwen3.5 · Veloce" -> "qwen3-5-veloce"); never empty. */
export function slugify(name: string): string {
  const s = name
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40)
    .replace(/-+$/g, "");
  return s || "profilo";
}

/** `base`, or `base-2`, `base-3`… when the id is taken. */
export function uniqueId(base: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  if (!used.has(base)) return base;
  for (let n = 2; ; n++) if (!used.has(`${base}-${n}`)) return `${base}-${n}`;
}

/** The profile with another model and runtime. A context larger than the model's window is brought back to it. */
export function applyModelToProfile(profile: InferenceProfile, model: Pick<LocalModel, "id" | "context_length">, runtimeId: string): InferenceProfile {
  const params = { ...profile.params };
  if (model.context_length && params.context && params.context > model.context_length) params.context = model.context_length;
  return { ...profile, model: model.id, runtime_id: runtimeId, params };
}

/** A new profile for the model with moderate defaults (the context never exceeds what the model supports). */
export function newProfileFor(model: Pick<LocalModel, "id" | "name" | "context_length">, runtimeId: string, name: string, takenIds: Iterable<string>): InferenceProfile {
  const clean = name.trim();
  const context = Math.min(model.context_length ?? 8192, 32768);
  return {
    id: uniqueId(slugify(clean), takenIds),
    name: clean,
    description: `${model.name}`,
    runtime_id: runtimeId,
    model: model.id,
    params: { temperature: 0.6, top_p: 0.95, max_tokens: 4096, context },
  };
}
