import type { CatalogModel, LocalModel } from "../../../shared/api/types";
import { fitRank } from "./fit";
import { isExternalOrigin } from "./origin";

/** Lowercase, accent-free text for forgiving searches ("città" matches "citta"). */
export function normalize(text: string): string {
  return text.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
}

/** True when every word of `query` appears somewhere in `haystack`. An empty query matches everything. */
export function matchesQuery(haystack: string, query: string): boolean {
  const words = normalize(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const hay = normalize(haystack);
  return words.every((w) => hay.includes(w));
}

export interface InstalledFilter {
  q: string;
  /** "" = every origin. */
  origin: string;
  /** "" = every format. */
  format: string;
}

export function filterInstalled(models: LocalModel[], f: InstalledFilter): LocalModel[] {
  return models.filter((m) => {
    if (f.origin && (m.origin || "manual") !== f.origin) return false;
    if (f.format && m.format !== f.format) return false;
    return matchesQuery(`${m.name} ${m.id} ${m.format} ${m.quant ?? ""} ${m.arch ?? ""} ${m.capabilities.join(" ")} ${m.source ?? ""}`, f.q);
  });
}

/** Distinct formats of the installed models, alphabetical. */
export function installedFormats(models: LocalModel[]): string[] {
  return [...new Set(models.map((m) => m.format).filter(Boolean))].sort();
}

/** How many models each origin has (origins with none are absent). */
export function originCounts(models: LocalModel[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const m of models) {
    const o = m.origin || "manual";
    out[o] = (out[o] ?? 0) + 1;
  }
  return out;
}

export interface InstalledSummary {
  total: number;
  /** Bytes inside AIW's own models folder (origin aiw or manual). */
  ownBytes: number;
  /** Bytes of models that belong to other apps and are only read. */
  externalBytes: number;
  externalCount: number;
}

export function summarizeInstalled(models: LocalModel[]): InstalledSummary {
  let ownBytes = 0;
  let externalBytes = 0;
  let externalCount = 0;
  for (const m of models) {
    if (isExternalOrigin(m.origin)) {
      externalBytes += m.size_bytes;
      externalCount++;
    } else ownBytes += m.size_bytes;
  }
  return { total: models.length, ownBytes, externalBytes, externalCount };
}

export interface CatalogFilter {
  q: string;
  /** Hide the entries that cannot run on this device. */
  fitOnly: boolean;
}

/** Filters the curated catalog and pushes what does not fit to the end (stable: curated order is kept inside a tier). */
export function filterCatalog(list: CatalogModel[], f: CatalogFilter): CatalogModel[] {
  const kept = list.filter((c) => {
    if (f.fitOnly && c.fit === "no") return false;
    return matchesQuery(`${c.name} ${c.family} ${c.publisher} ${c.format} ${c.quant} ${c.tags.join(" ")} ${c.capabilities.join(" ")}`, f.q);
  });
  return kept
    .map((c, i) => ({ c, i }))
    .sort((a, b) => fitRank(a.c.fit) - fitRank(b.c.fit) || a.i - b.i)
    .map((x) => x.c);
}
