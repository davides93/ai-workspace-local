import type { DownloadProgress, DownloadStatus, PullRequest } from "../../../shared/api/types";
import { formatDuration } from "../../../shared/lib/format";
import type { LedTone, PillTone } from "../../../shared/ui";

/** Waiting, transferring or verifying: the job can still be cancelled and the list must keep polling. */
export function isActiveJob(d: Pick<DownloadProgress, "status">): boolean {
  return d.status === "queued" || d.status === "running" || d.status === "verifying";
}

export const DOWNLOAD_STATUS: Record<DownloadStatus, { label: string; tone: PillTone; led: LedTone }> = {
  queued: { label: "In coda", tone: "neutral", led: "off" },
  running: { label: "In download", tone: "run", led: "run" },
  verifying: { label: "Verifica sha256", tone: "run", led: "run" },
  done: { label: "Completato", tone: "ok", led: "ok" },
  error: { label: "Errore", tone: "crit", led: "crit" },
  cancelled: { label: "Annullato", tone: "neutral", led: "off" },
};

/** Name shown for a job: the repository or model reference, else the model id. */
export function jobName(d: Pick<DownloadProgress, "model_ref" | "model_id" | "file">): string {
  return d.model_ref || d.model_id || d.file;
}

/** 0..1 share of the aggregated transfer (all files of the job). */
export function progressRatio(d: Pick<DownloadProgress, "received" | "total" | "status">): number {
  if (d.status === "done") return 1;
  if (!d.total || d.total <= 0) return 0;
  return Math.max(0, Math.min(1, d.received / d.total));
}

/** Seconds left at the current speed, or null when it cannot be told (not running, no speed, nothing left). */
export function etaSeconds(d: Pick<DownloadProgress, "received" | "total" | "speed_bps" | "status">): number | null {
  if (d.status !== "running") return null;
  if (!d.speed_bps || d.speed_bps <= 0) return null;
  const left = d.total - d.received;
  if (!Number.isFinite(left) || left <= 0) return null;
  return left / d.speed_bps;
}

/** "pochi secondi", "35 s", "circa 4 min", "circa 1 h 22 min". */
export function formatEta(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds)) return "—";
  if (seconds < 10) return "pochi secondi";
  if (seconds < 60) return formatDuration(seconds);
  return `circa ${formatDuration(seconds)}`;
}

export interface QueueSummary {
  active: number;
  running: number;
  queued: number;
  verifying: number;
  /** Jobs that ended in error: they stay on screen until dismissed. */
  failed: number;
  /** Done or cancelled jobs that can be cleared in one go. */
  finished: number;
  speedBps: number;
  remainingBytes: number;
  etaSeconds: number | null;
}

export function summarizeQueue(jobs: DownloadProgress[]): QueueSummary {
  const s: QueueSummary = { active: 0, running: 0, queued: 0, verifying: 0, failed: 0, finished: 0, speedBps: 0, remainingBytes: 0, etaSeconds: null };
  for (const d of jobs) {
    if (isActiveJob(d)) {
      s.active++;
      s.remainingBytes += Math.max(0, d.total - d.received);
      if (d.status === "running") {
        s.running++;
        s.speedBps += d.speed_bps || 0;
      } else if (d.status === "queued") s.queued++;
      else s.verifying++;
    } else if (d.status === "error") s.failed++;
    else s.finished++;
  }
  s.etaSeconds = s.speedBps > 0 && s.remainingBytes > 0 ? s.remainingBytes / s.speedBps : null;
  return s;
}

const SORT_GROUP: Record<DownloadStatus, number> = { running: 0, verifying: 0, queued: 1, error: 2, cancelled: 3, done: 3 };

/** Active jobs first (transferring, then waiting), then the failed ones, then the finished; the server order is kept inside a group. */
export function sortJobs(jobs: DownloadProgress[]): DownloadProgress[] {
  return jobs
    .map((d, i) => ({ d, i }))
    .sort((a, b) => (SORT_GROUP[a.d.status] ?? 4) - (SORT_GROUP[b.d.status] ?? 4) || a.i - b.i)
    .map((x) => x.d);
}

/** What "Riprova" does for a job: pull the same request again, reopen the URL form, or nothing at all. */
export type RetryPlan = { kind: "pull"; request: string | PullRequest } | { kind: "url"; url: string } | null;

const HUB_PROVIDERS = new Set(["hf", "modelscope", "ollama"]);

/**
 * Rebuilds the request of a job from `provider`, `model_ref` and `variant` (docs/architecture/model-hub.md §9):
 * a curated catalog entry is pulled again by its id; a hub model by `{provider, model, variant}`.
 * A direct URL cannot be rebuilt faithfully (the optional sha256 is not on the job), so the form is
 * reopened with the URL filled in. Anything else gets no retry.
 */
export function retryPlan(job: DownloadProgress, catalogIds: ReadonlySet<string>): RetryPlan {
  if (job.status !== "error" && job.status !== "cancelled") return null;
  if (catalogIds.has(job.model_id)) return { kind: "pull", request: job.model_id };
  if (job.provider === "url") return /^https?:\/\//i.test(job.model_ref) ? { kind: "url", url: job.model_ref } : null;
  if (HUB_PROVIDERS.has(job.provider) && job.model_ref) {
    return { kind: "pull", request: { provider: job.provider, model: job.model_ref, variant: job.variant } };
  }
  return null;
}

/** "file 2 di 3" while transferring, "3 file" once done; nothing for a single-file job. */
export function filesLabel(d: Pick<DownloadProgress, "files_done" | "files_total" | "status">): string | null {
  if (d.files_total <= 1) return null;
  if (d.status === "done") return `${d.files_total} file`;
  const current = Math.min(d.files_done + (isActiveJob(d) ? 1 : 0), d.files_total);
  return `file ${current} di ${d.files_total}`;
}
