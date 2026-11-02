import type { DownloadProgress, ModelVariant, RemoteFile, RuntimeCompat } from "../../../shared/api/types";
import { isActiveJob } from "./downloads";

/**
 * Recommended variant first, then the rest from the smallest to the largest download
 * (which is also the order of quality for quantizations). Never mutates its input.
 */
export function sortVariants(variants: ModelVariant[]): ModelVariant[] {
  return variants
    .map((v, i) => ({ v, i }))
    .sort((a, b) => Number(b.v.recommended) - Number(a.v.recommended) || a.v.size_bytes - b.v.size_bytes || a.i - b.i)
    .map((x) => x.v);
}

export interface VariantGroup {
  format: string;
  variants: ModelVariant[];
}

/**
 * Variants grouped by format. The group that holds the recommended variant comes first, the others
 * keep the order in which the provider listed them; inside a group see `sortVariants`.
 */
export function groupVariants(variants: ModelVariant[]): VariantGroup[] {
  const order: string[] = [];
  const by = new Map<string, ModelVariant[]>();
  for (const v of variants) {
    if (!by.has(v.format)) {
      by.set(v.format, []);
      order.push(v.format);
    }
    by.get(v.format)!.push(v);
  }
  const groups = order.map((format) => ({ format, variants: sortVariants(by.get(format)!) }));
  return groups
    .map((g, i) => ({ g, i }))
    .sort((a, b) => Number(b.g.variants.some((v) => v.recommended)) - Number(a.g.variants.some((v) => v.recommended)) || a.i - b.i)
    .map((x) => x.g);
}

/** Optionally hides the variants that cannot run on this device (the installed and recommended ones always stay). */
export function visibleVariants(variants: ModelVariant[], fitOnly: boolean): ModelVariant[] {
  if (!fitOnly) return variants;
  return variants.filter((v) => v.fit !== "no" || v.recommended || v.installed);
}

export interface FileSummary {
  count: number;
  weights: number;
  hasMmproj: boolean;
}

export function summarizeFiles(files: RemoteFile[]): FileSummary {
  return {
    count: files.length,
    weights: files.filter((f) => f.role === "weights").length,
    hasMmproj: files.some((f) => f.role === "mmproj"),
  };
}

/** "1 file", "3 file", "2 file, con mmproj" — the projector of a vision model counts as a file. */
export function fileCountLabel(files: RemoteFile[]): string {
  const s = summarizeFiles(files);
  if (s.count === 0) return "nessun file";
  return `${s.count} file${s.hasMmproj ? ", con mmproj" : ""}`;
}

export const FILE_ROLE_LABEL: Record<RemoteFile["role"], string> = {
  weights: "pesi",
  mmproj: "mmproj",
  config: "config",
  tokenizer: "tokenizer",
  other: "altro",
};

export interface RuntimeSplit {
  available: RuntimeCompat[];
  unavailable: RuntimeCompat[];
}

/** Splits the compatible runtimes into the ones ready on this device and the ones that are not (with their reason). */
export function splitRuntimes(runtimes: RuntimeCompat[]): RuntimeSplit {
  return {
    available: runtimes.filter((r) => r.available),
    unavailable: runtimes.filter((r) => !r.available),
  };
}

/**
 * The runtimes of a group of variants when every variant lists exactly the same ones with the same
 * availability: the drawer then says it once above the list instead of repeating it on every row.
 */
export function sharedRuntimes(variants: ModelVariant[]): RuntimeCompat[] | null {
  if (variants.length < 2) return null;
  const key = (v: ModelVariant) => v.runtimes.map((r) => `${r.id}:${r.available ? 1 : 0}`).sort().join("|");
  const first = key(variants[0]);
  return variants.every((v) => key(v) === first) ? variants[0].runtimes : null;
}

/** The active job downloading `(provider, model, variant)`, if any; the job of the recommended variant may carry an empty `variant`. */
export function findJob(jobs: DownloadProgress[], ref: { provider: string; model: string; variant: string }): DownloadProgress | undefined {
  return jobs.find((d) => isActiveJob(d) && d.provider === ref.provider && d.model_ref === ref.model && (d.variant === ref.variant || d.variant === ""));
}
