import type { DeviceProfile, DeviceTier } from "../../../shared/api/types";

/** What each device class can run, as the catalog recommends it (DESIGN §Dispositivi). */
export const TIER_INFO: Record<DeviceTier, { label: string; models: string }> = {
  xs: { label: "Molto piccolo", models: "Modelli di sistema e fino a 1–2B (Gemma 4 E2B, Qwen3 0.6B)" },
  s: { label: "Piccolo", models: "Fino a circa 4B quantizzati (Gemma 4 E4B, Phi-4 mini)" },
  m: { label: "Medio", models: "7–14B e MoE con pochi parametri attivi" },
  l: { label: "Grande", models: "MoE fino a circa 35B (A3B) e densi fino a 27–32B" },
  xl: { label: "Molto grande", models: "Densi da 70B, MoE da oltre 100B con offload" },
};

export const TIER_ORDER: DeviceTier[] = ["xs", "s", "m", "l", "xl"];

/** Share of RAM a model may use on a device where CPU and GPU share memory (`aiw-hw` UNIFIED_MEMORY_PERCENT). */
export const UNIFIED_MEMORY_PERCENT = 70;

/** Largest dedicated accelerator memory (the CPU does not count), in MB. */
export function vramMb(d: Pick<DeviceProfile, "accelerators">): number {
  return d.accelerators.filter((a) => a.kind !== "cpu").reduce((max, a) => Math.max(max, a.memory_mb ?? 0), 0);
}

/** Apple Silicon and phones share memory between CPU and GPU; an Intel Mac does not (`DeviceProfile::unified_memory`). */
export function isUnifiedMemory(d: Pick<DeviceProfile, "os" | "arch" | "accelerators">): boolean {
  return d.os === "ios" || d.os === "android" || (d.os === "macos" && d.arch === "aarch64" && d.accelerators.some((a) => a.kind === "metal"));
}

/**
 * Memory a model can use (weights + KV cache), in MB: the same rule as `aiw_hw::model_budget_mb`,
 * which also decides the device class and the "Ci sta" labels of the catalog.
 */
export function modelBudgetMb(d: Pick<DeviceProfile, "os" | "arch" | "accelerators" | "memory_mb">): number {
  if (isUnifiedMemory(d)) {
    const share = Math.floor((d.memory_mb * UNIFIED_MEMORY_PERCENT) / 100);
    const gpu = vramMb(d);
    return gpu === 0 ? share : Math.min(gpu, share);
  }
  return Math.max(vramMb(d), Math.floor(d.memory_mb / 2));
}

/** One sentence that says where the budget comes from. */
export function budgetRule(d: Pick<DeviceProfile, "os" | "arch" | "accelerators" | "memory_mb">): string {
  if (isUnifiedMemory(d)) return `Memoria unificata: i modelli possono usare al massimo il ${UNIFIED_MEMORY_PERCENT}% della RAM.`;
  const vram = vramMb(d);
  if (vram === 0) return "Nessuna GPU dedicata: i modelli girano su CPU e possono usare metà della RAM.";
  return vram >= d.memory_mb / 2
    ? "La memoria della GPU è il limite: i modelli che ci stanno girano interamente su GPU."
    : "Poca memoria sulla GPU: i modelli più grandi vanno in parte su CPU, usando metà della RAM.";
}

export type MemoryTone = "ok" | "warn" | "crit";

/** RAM in use as a ratio and the tone of its meter (amber above 70 %, red above 85 %). */
export function memoryUsage(d: Pick<DeviceProfile, "memory_mb" | "available_memory_mb">): { used: number; ratio: number; tone: MemoryTone } {
  const used = Math.max(0, d.memory_mb - d.available_memory_mb);
  const ratio = d.memory_mb > 0 ? used / d.memory_mb : 0;
  return { used, ratio, tone: ratio > 0.85 ? "crit" : ratio > 0.7 ? "warn" : "ok" };
}
