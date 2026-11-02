import type { CatalogModel, DownloadProgress } from "../../../shared/api/types";
import { formatBytes } from "../../../shared/lib/format";
import { isActiveJob } from "./downloads";

/** Entry that points to a whole repository instead of one file (the MLX entry): `source.file` is empty. */
export function isWholeRepo(c: Pick<CatalogModel, "source">): boolean {
  return c.source.kind === "hf" && c.source.file === "";
}

/** Size of the download, or null when there is nothing to download (system models). */
export function catalogSize(c: Pick<CatalogModel, "size_bytes" | "downloadable">): string | null {
  return c.downloadable && c.size_bytes > 0 ? formatBytes(c.size_bytes) : null;
}

/** Chips of a catalog card: format, quantization and, for a whole-repository entry, what that means. */
export function catalogTags(c: CatalogModel): string[] {
  const tags = [`.${c.format}`];
  if (c.quant) tags.push(c.quant);
  if (isWholeRepo(c)) tags.push("repository intero");
  return tags;
}

/** Runtime of a system model (apple-fm, gemini-nano, phi-silica), when it is one. */
export function systemRuntime(c: Pick<CatalogModel, "source">): string | null {
  return c.source.kind === "system" ? c.source.runtime : null;
}

export type CatalogState = "system" | "installed" | "downloading" | "blocked" | "ready";

/** What the footer of a catalog card offers; the order of the checks is the priority of the states. */
export function catalogState(c: CatalogModel, job: DownloadProgress | undefined): CatalogState {
  if (!c.downloadable) return "system";
  if (c.installed) return "installed";
  if (job) return "downloading";
  if (c.fit === "no") return "blocked";
  return "ready";
}

/** The active download of a catalog entry: its job keeps the catalog id as `model_id`. */
export function findCatalogJob(jobs: DownloadProgress[], c: Pick<CatalogModel, "id">): DownloadProgress | undefined {
  return jobs.find((d) => d.model_id === c.id && isActiveJob(d));
}

/** Platform requirement worth saying out loud on the card (MLX runs only on macOS with Apple Silicon). */
export function platformNote(c: Pick<CatalogModel, "format" | "downloadable">): string | null {
  if (c.format === "mlx" && c.downloadable) return "Formato MLX: si scarica il repository intero e gira solo su macOS con Apple Silicon.";
  return null;
}

/** Where a system model comes from, in a sentence. */
export function systemNote(runtime: string | null): string {
  const where =
    runtime === "apple-fm" ? "da Apple Intelligence su iPhone e Mac"
    : runtime === "gemini-nano" ? "da Android (AICore) sui telefoni che lo supportano"
    : runtime === "phi-silica" ? "da Windows sui PC Copilot+"
    : "dal sistema operativo";
  return `Viene fornito ${where}: non si scarica e non occupa spazio nella cartella dei modelli. Compare tra i runtime se questo device lo include.`;
}
