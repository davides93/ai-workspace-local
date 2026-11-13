import type { Fit, InstallJob, LocalModel, RuntimeInfo, RuntimeKind, SettingsSchema } from "../../../shared/api/types";
import { t, tFields } from "../../../shared/i18n";
import type { PillTone } from "../../../shared/ui";

export const KIND_ORDER: RuntimeKind[] = ["sidecar", "embedded", "system", "remote"];

export const KIND_META: Record<RuntimeKind, { title: string; desc: string }> = {
  sidecar: tFields({}, { title: "runtimes.kind_sidecar", desc: "runtimes.kind_sidecar_desc" }),
  embedded: tFields({}, { title: "runtimes.kind_embedded", desc: "runtimes.kind_embedded_desc" }),
  system: tFields({}, { title: "runtimes.kind_system", desc: "runtimes.kind_system_desc" }),
  remote: tFields({}, { title: "runtimes.kind_remote", desc: "runtimes.kind_remote_desc" }),
};

export type Scope = "here" | "all";

/** A runtime is usable on this device unless it is flagged unsupported (wrong OS or no accelerator). */
export const isUsableHere = (r: Pick<RuntimeInfo, "status">) => r.status !== "unsupported";

/** Case-insensitive match on name, id, vendor, description, kind, accelerators and formats. */
export function matchesRuntime(r: RuntimeInfo, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [r.name, r.id, r.vendor ?? "", r.description, r.kind, ...r.accelerators, ...r.formats].some((s) => s.toLowerCase().includes(q));
}

export function filterRuntimes(list: RuntimeInfo[], f: { scope: Scope; query: string }): RuntimeInfo[] {
  return list.filter((r) => (f.scope === "all" || isUsableHere(r)) && matchesRuntime(r, f.query));
}

/** Runtimes by kind in the fixed display order; kinds without runtimes are left out. */
export function groupByKind(list: RuntimeInfo[]): { kind: RuntimeKind; items: RuntimeInfo[] }[] {
  return KIND_ORDER.map((kind) => ({ kind, items: list.filter((r) => r.kind === kind) })).filter((g) => g.items.length > 0);
}

export function runtimeCounts(list: RuntimeInfo[]): { running: number; usable: number; total: number } {
  return { running: list.filter((r) => r.status === "running").length, usable: list.filter(isUsableHere).length, total: list.length };
}

/** How well an installed model fits this device, as label and pill tone. */
/** How a runtime talks: `none` is an in-process engine with no HTTP API of its own. */
export function apiLabel(api: RuntimeInfo["api"] | string): string {
  return api === "none" ? t("runtimes.api_none") : t("runtimes.api_named", { api });
}

export const FIT_META: Record<Fit, { label: string; tone: PillTone }> = {
  great: tFields({ tone: "ok" as const }, { label: "runtimes.fit_great" }),
  ok: tFields({ tone: "info" as const }, { label: "runtimes.fit_ok" }),
  tight: tFields({ tone: "warn" as const }, { label: "runtimes.fit_tight" }),
  no: tFields({ tone: "crit" as const }, { label: "runtimes.fit_no" }),
};

const FIT_RANK: Record<Fit | "", number> = { great: 0, ok: 1, tight: 2, no: 3, "": 4 };

/**
 * Installed models this runtime can run, best fit first. A model counts when its compatibility
 * entry for the runtime is available; models without entries fall back to the plain id list.
 */
export function compatibleModels(models: LocalModel[], runtimeId: string): LocalModel[] {
  return models
    .filter((m) => {
      const c = m.runtime_compat?.find((x) => x.id === runtimeId);
      return c ? c.available : m.runtimes.includes(runtimeId);
    })
    .sort((a, b) => FIT_RANK[a.fit] - FIT_RANK[b.fit] || a.name.localeCompare(b.name));
}

/** The value of every setting of the schema at its default. */
export function defaultSettings(schema: SettingsSchema): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [k, f] of Object.entries(schema.properties)) if (f.default !== undefined) out[k] = f.default;
  return out;
}

/** Saved settings with the schema defaults filled in: what the form shows, so drafts compare fairly. */
export function effectiveSettings(schema: SettingsSchema, settings: Record<string, unknown>): Record<string, unknown> {
  return { ...defaultSettings(schema), ...settings };
}

const stable = (o: Record<string, unknown>) => JSON.stringify(Object.keys(o).sort().map((k) => [k, o[k]]));

/** Did the user change anything compared with what is saved? Untouched defaults do not count. */
export function settingsDirty(schema: SettingsSchema, saved: Record<string, unknown>, draft: Record<string, unknown>): boolean {
  return stable(effectiveSettings(schema, saved)) !== stable(effectiveSettings(schema, draft));
}

/** Progress of an installation as a fraction of its steps: the one that runs counts as half done. */
export function installProgress(job: Pick<InstallJob, "state" | "step" | "steps">): { value: number; max: number } {
  const max = Math.max(job.steps, 1);
  if (job.state === "done") return { value: max, max };
  return { value: Math.min(Math.max(job.step - 1, 0) + (job.state === "running" ? 0.5 : 0), max), max };
}

/** The last `n` log lines (a long log would make the box and the DOM huge). */
export const logTail = (lines: string[], n = 300) => (lines.length > n ? lines.slice(-n) : lines);
