import type { Fit, LocalModel, RuntimeInfo, RuntimeKind, SettingsSchema } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";

export const KIND_ORDER: RuntimeKind[] = ["sidecar", "embedded", "system", "remote"];

export const KIND_META: Record<RuntimeKind, { title: string; desc: string }> = {
  sidecar: { title: "Sidecar gestiti", desc: "Processi avviati, monitorati e fermati dall'app; parlano HTTP compatibile OpenAI." },
  embedded: { title: "Embedded", desc: "Librerie collegate in-process via FFI: nessun processo esterno; sono il default su mobile." },
  system: { title: "Modelli di sistema", desc: "Modelli già presenti nel sistema operativo, condivisi tra le app." },
  remote: { title: "Remoti in LAN", desc: "Runtime di altri dispositivi AIW o server compatibili nella tua rete." },
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
  return api === "none" ? "senza API" : `API ${api}`;
}

export const FIT_META: Record<Fit, { label: string; tone: PillTone }> = {
  great: { label: "Ci sta comodo", tone: "ok" },
  ok: { label: "Ci sta", tone: "info" },
  tight: { label: "Al limite", tone: "warn" },
  no: { label: "Non ci sta", tone: "crit" },
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

/** The last `n` log lines (a long log would make the box and the DOM huge). */
export const logTail = (lines: string[], n = 300) => (lines.length > n ? lines.slice(-n) : lines);
