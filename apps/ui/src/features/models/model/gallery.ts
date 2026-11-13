import type { CatalogModel, LocalModel } from "../../../shared/api/types";

/** Models for a phone or tablet: the ones the OS provides plus the catalog entries tagged `mobile`. */
export function isGalleryModel(c: Pick<CatalogModel, "tags" | "downloadable" | "source" | "format">): boolean {
  if (c.format === "embeddings" || c.tags.includes("embeddings") || c.format === "ggml" || c.format === "piper") return false;
  if (!c.downloadable) return c.source.kind === "system" && c.source.runtime !== "phi-silica";
  return c.tags.includes("mobile");
}

/** System models first (nothing to download), then the lightest. */
export function galleryModels(list: CatalogModel[]): CatalogModel[] {
  return list
    .filter(isGalleryModel)
    .sort((a, b) => Number(a.downloadable) - Number(b.downloadable) || a.params_b - b.params_b || a.name.localeCompare(b.name));
}

/** The local model that a downloaded catalog entry became: matched by its file name. */
export function findInstalled(c: Pick<CatalogModel, "source" | "downloadable">, models: LocalModel[]): LocalModel | undefined {
  if (!c.downloadable) return undefined;
  if (c.source.kind !== "hf" || !c.source.file) return undefined;
  const file = c.source.file;
  return models.find((m) => m.path === file || m.path?.endsWith(`/${file}`) || m.path?.endsWith(`\\${file}`));
}

/** The local model behind a system catalog entry (same id). */
export function findSystem(c: Pick<CatalogModel, "id" | "downloadable">, models: LocalModel[]): LocalModel | undefined {
  return c.downloadable ? undefined : models.find((m) => m.id === c.id && m.origin === "system");
}
