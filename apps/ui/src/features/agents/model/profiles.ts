import { currentLocaleTag } from "../../../shared/lib/format";
// Inference profiles: which models a runtime can serve, defaults, ids and checks. Pure logic.

import type { InferenceProfile, LocalModel, RuntimeInfo } from "../../../shared/api/types";
import { isThinkingMode } from "../../../shared/lib/thinking";
import { slugify, uniqueId } from "./agents";

import { t as tr, t } from "../../../shared/i18n";
/** A speech model (Whisper to listen, a Piper voice to speak): it serves the voice mode, never a chat profile. */
export const isSpeechOnly = (capabilities: readonly string[]): boolean => capabilities.length > 0 && capabilities.every((c) => c === "stt" || c === "tts");

/** Runtimes a chat profile can use (not the speech-only ones, like whisper.cpp). */
export function chatRuntimes<T extends { capabilities: readonly string[] }>(runtimes: readonly T[]): T[] {
  return runtimes.filter((r) => !isSpeechOnly(r.capabilities));
}

/** The runtime of a profile that leaves the choice to the router (decided when a run starts). */
export const AUTO_RUNTIME = "auto";

/** Models the runtime can serve, by `runtime_compat` (the same list the Models page shows); speech models are left out. With `auto` every chat model is offered. */
export function compatibleModels(models: readonly LocalModel[], runtimeId: string): LocalModel[] {
  if (runtimeId === AUTO_RUNTIME) return models.filter((m) => !isSpeechOnly(m.capabilities));
  return models.filter((m) => !isSpeechOnly(m.capabilities) && (m.runtime_compat.some((c) => c.id === runtimeId) || m.runtimes.includes(runtimeId)));
}

/** Why a model cannot run on a runtime ("" when it can, or when nothing is known). */
export function incompatibility(model: LocalModel | undefined, runtimeId: string): string {
  if (!model) return "";
  const c = model.runtime_compat.find((x) => x.id === runtimeId);
  if (c) return c.available ? "" : c.reason;
  return model.runtimes.includes(runtimeId) ? "" : t("agents.questo_runtime_non_apre_il");
}

/** First model to propose after the runtime changed: the current one when it still fits, else the best compatible one. */
export function modelAfterRuntimeChange(current: string, models: readonly LocalModel[], runtimeId: string): string {
  const fits = compatibleModels(models, runtimeId);
  if (fits.some((m) => m.id === current)) return current;
  return fits[0]?.id ?? "";
}

export const CONTEXT_PRESETS = [2048, 4096, 8192, 16384, 32768, 65536, 131072] as const;

export function blankProfile(runtimeId = "", model = ""): InferenceProfile {
  return { id: "", name: "", description: "", runtime_id: runtimeId, model, params: { temperature: 0.6, context: 8192, max_tokens: 2048 } };
}

export function idForProfile(name: string, existing: readonly InferenceProfile[]): string {
  return uniqueId(slugify(name, "profilo"), existing.map((p) => p.id));
}

/** Empty optional fields are left out so the backend keeps its own defaults. */
export function normalizeProfile(p: InferenceProfile): InferenceProfile {
  const params: InferenceProfile["params"] = {};
  const put = <K extends "temperature" | "top_p" | "max_tokens" | "context">(k: K, v: number | undefined) => {
    if (v !== undefined && Number.isFinite(v)) params[k] = v;
  };
  put("temperature", p.params.temperature);
  put("top_p", p.params.top_p);
  put("max_tokens", p.params.max_tokens === undefined ? undefined : Math.round(p.params.max_tokens));
  put("context", p.params.context === undefined ? undefined : Math.round(p.params.context));
  if (isThinkingMode(p.params.thinking)) params.thinking = p.params.thinking;
  const budget = p.params.thinking_budget;
  if (budget !== undefined && Number.isFinite(budget) && budget >= 1) params.thinking_budget = Math.round(budget);
  // Only with the automatic runtime; adapters need a runtime that loads them at start.
  if (p.runtime_id === AUTO_RUNTIME && p.params.allow_remote) params.allow_remote = true;
  const adapters = p.runtime_id === AUTO_RUNTIME ? [] : (p.params.adapters ?? [])
    .map((a) => ({ path: a.path.trim(), scale: a.scale !== undefined && Number.isFinite(a.scale) ? a.scale : undefined }))
    .filter((a) => a.path !== "")
    .map((a) => (a.scale === undefined ? { path: a.path } : a));
  if (adapters.length > 0) params.adapters = adapters;
  const description = p.description?.trim();
  return { ...p, name: p.name.trim(), description: description ? description : undefined, model: p.model.trim(), params };
}

export interface ProfileIssue {
  field: "name" | "runtime_id" | "model" | "temperature" | "top_p" | "context" | "max_tokens" | "thinking_budget" | "adapters";
  level: "error" | "warn";
  message: string;
}

export function validateProfile(p: InferenceProfile, ctx: { runtimes: readonly RuntimeInfo[]; models: readonly LocalModel[] }): ProfileIssue[] {
  const issues: ProfileIssue[] = [];
  if (!p.name.trim()) issues.push({ field: "name", level: "error", message: tr("agents.dai_un_nome_al_profilo") });
  if (!p.runtime_id) issues.push({ field: "runtime_id", level: "error", message: tr("agents.scegli_il_runtime") });
  if (!p.model.trim()) issues.push({ field: "model", level: "error", message: tr("agents.scegli_il_modello") });
  const t = p.params.temperature;
  if (t !== undefined && (!Number.isFinite(t) || t < 0 || t > 2)) issues.push({ field: "temperature", level: "error", message: tr("agents.la_temperatura_va_da_0") });
  const tp = p.params.top_p;
  if (tp !== undefined && (!Number.isFinite(tp) || tp <= 0 || tp > 1)) issues.push({ field: "top_p", level: "error", message: tr("agents.top_p_va_da_0") });
  const c = p.params.context;
  if (c !== undefined && (!Number.isFinite(c) || c < 512)) issues.push({ field: "context", level: "error", message: tr("agents.il_contesto_minimo_e_512") });
  const m = p.params.max_tokens;
  if (m !== undefined && (!Number.isFinite(m) || m < 1)) issues.push({ field: "max_tokens", level: "error", message: tr("agents.indica_almeno_1_token_di") });
  if (c !== undefined && m !== undefined && Number.isFinite(c) && Number.isFinite(m) && m >= c) {
    issues.push({ field: "max_tokens", level: "warn", message: tr("agents.la_risposta_massima_e_pari") });
  }
  const tb = p.params.thinking_budget;
  if (tb !== undefined && (!Number.isFinite(tb) || tb < 1)) issues.push({ field: "thinking_budget", level: "error", message: tr("agents.indica_almeno_1_token_di_2") });
  const named = (p.params.adapters ?? []).filter((a) => a.path.trim() !== "");
  const runtime = ctx.runtimes.find((r) => r.id === p.runtime_id);
  if (named.length > 0 && p.runtime_id === AUTO_RUNTIME) {
    issues.push({ field: "adapters", level: "error", message: tr("agents.gli_adapter_si_caricano_all") });
  } else if (named.length > 0 && runtime && !runtime.adapter_format) {
    issues.push({ field: "adapters", level: "error", message: tr("agents.non_carica_adapter_lora_da", { name: runtime.name }) });
  }
  if ((p.params.adapters ?? []).some((a) => a.scale !== undefined && (!Number.isFinite(a.scale) || a.scale <= 0))) {
    issues.push({ field: "adapters", level: "error", message: tr("agents.la_scala_dell_adapter_deve") });
  }
  const model = ctx.models.find((x) => x.id === p.model);
  if (model && p.runtime_id && p.runtime_id !== AUTO_RUNTIME) {
    const why = incompatibility(model, p.runtime_id);
    if (why) issues.push({ field: "model", level: "warn", message: tr("agents.il_modello_non_e_compatibile", { why }) });
  }
  if (model?.context_length && c !== undefined && Number.isFinite(c) && c > model.context_length) {
    issues.push({ field: "context", level: "warn", message: tr("agents.il_modello_supporta_al_massimo", { count: model.context_length.toLocaleString(currentLocaleTag()) }) });
  }
  return issues;
}

/** Fields the simple form does not show: a problem in one of them must be said, or "Salva" is off for no visible reason. */
const SIMPLE_FIELDS: ProfileIssue["field"][] = ["name", "model"];

export const hiddenProfileErrors = (issues: readonly ProfileIssue[]): ProfileIssue[] =>
  issues.filter((i) => i.level === "error" && !SIMPLE_FIELDS.includes(i.field));

export const hasProfileErrors = (issues: readonly ProfileIssue[]): boolean => issues.some((i) => i.level === "error");
