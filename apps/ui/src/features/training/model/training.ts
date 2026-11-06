import type { AdapterInfo, DatasetSpec, TrainDoctor, TrainJob, TrainJobRequest, TrainMethod, TrainerInfo } from "../../../shared/api/types";

/** Poll interval of jobs and log, used only while a job is going. */
export const TRAIN_POLL_MS = 2_000;

/** Fewest examples (pairs for DPO) a dataset needs; the engine refuses fewer. */
export const MIN_EXAMPLES = 10;

export const METHOD_LABEL: Record<TrainMethod, string> = { sft: "SFT", dpo: "DPO", grpo: "GRPO" };

/** The methods the page offers. */
export const METHODS: TrainMethod[] = ["sft"];

export const METHOD_HINT: Record<TrainMethod, string> = {
  sft: "Impara dalle conversazioni approvate.",
  dpo: "Impara dalle coppie risposta buona / risposta scartata.",
  grpo: "Impara dai punteggi dei grader di una suite.",
};

export const SOURCE_LABEL: Record<string, string> = {
  feedback: "Pollice in su",
  succeeded: "Sessioni riuscite",
  eval: "Task superati nelle valutazioni",
};

/** The job is still going: the page polls it. */
export const isActive = (j: Pick<TrainJob, "status">): boolean => j.status === "queued" || j.status === "running";

/** Progress as a whole percentage, or `null` when the trainer has not printed any. */
export const jobPercent = (j: Pick<TrainJob, "progress" | "status">): number | null => {
  if (j.status === "succeeded") return 100;
  return j.progress === undefined ? null : Math.round(Math.min(1, Math.max(0, j.progress)) * 100);
};

/** Trainers that can run `method` here right now (installed, and the device is enough). */
export function readyTrainers(doctor: TrainDoctor | undefined, method: TrainMethod): TrainerInfo[] {
  return (doctor?.trainers ?? []).filter((t) => t.state === "ready" && t.methods.some((m) => m.method === method && !m.blocked));
}

/** Why `method` cannot start with the trainers of this device, in words; `null` when it can. */
export function methodBlocker(doctor: TrainDoctor | undefined, method: TrainMethod): string | null {
  if (!doctor) return null;
  if (readyTrainers(doctor, method).length > 0) return null;
  const knowing = doctor.trainers.filter((t) => t.methods.some((m) => m.method === method));
  if (knowing.length === 0) return `Nessun trainer conosce ${METHOD_LABEL[method]}.`;
  const missing = knowing.find((t) => t.state === "missing" && t.methods.some((m) => m.method === method && !m.blocked));
  if (missing) return `${missing.name} non è installato.`;
  const why = knowing.map((t) => t.methods.find((m) => m.method === method)?.blocked ?? t.reason).find(Boolean);
  return why ?? `Questo dispositivo non può fare ${METHOD_LABEL[method]}.`;
}

/** `0.84` → "0,84"; text and other values are shown as they are. */
export function metricText(v: unknown): string {
  return typeof v === "number" ? (Number.isInteger(v) ? String(v) : v.toFixed(2).replace(".", ",")) : String(v);
}

const METRIC_LABEL: Record<string, string> = { train_loss: "loss", val_loss: "loss di validazione", steps: "passi", duration_s: "secondi" };

/** "loss 0,84 · loss di validazione 0,97 · 51 passi" for the metrics a job or adapter reports. */
export function metricsLine(metrics: Record<string, unknown> | undefined): string {
  if (!metrics) return "";
  const parts: string[] = [];
  for (const k of ["train_loss", "val_loss"]) if (typeof metrics[k] === "number") parts.push(`${METRIC_LABEL[k]} ${metricText(metrics[k])}`);
  if (typeof metrics.steps === "number") parts.push(`${metrics.steps} passi`);
  if (typeof metrics.duration_s === "number") parts.push(`${Math.round(metrics.duration_s)} s`);
  return parts.join(" · ");
}

export interface DatasetForm {
  name: string;
  method: TrainMethod;
  agent: string;
  sources: string[];
  since: string;
  until: string;
  limit: string;
  evalReports: string;
}

export const blankDatasetForm = (method: TrainMethod = "sft"): DatasetForm => ({
  name: "", method, agent: "", sources: ["feedback"], since: "", until: "", limit: "", evalReports: "",
});

/** The request for the form, or the first thing wrong with it. */
export function datasetSpec(f: DatasetForm): { spec: DatasetSpec; error?: undefined } | { spec?: undefined; error: string } {
  const limit = f.limit.trim();
  if (limit && !(Number.isInteger(Number(limit)) && Number(limit) >= 1)) return { error: "Il limite deve essere un numero intero da 1 in su." };
  if (f.method === "sft" && f.sources.length === 0) return { error: "Scegli almeno una sorgente." };
  const reports = f.evalReports.split(/[\s,]+/).filter(Boolean);
  if (f.sources.includes("eval") && reports.length === 0) return { error: "Per i task superati indica almeno un report di valutazione." };
  const spec: DatasetSpec = { method: f.method };
  if (f.name.trim()) spec.name = f.name.trim();
  if (f.agent) spec.agent = f.agent;
  if (f.since.trim()) spec.since = f.since.trim();
  if (f.until.trim()) spec.until = f.until.trim();
  if (limit) spec.limit = Number(limit);
  if (f.method === "sft") spec.sources = f.sources;
  if (reports.length > 0) spec.eval_reports = reports;
  return { spec };
}

export interface JobForm {
  method: TrainMethod;
  trainer: string;
  base: string;
  dataset: string;
  name: string;
  iters: string;
  epochs: string;
  rank: string;
  lr: string;
  batch: string;
  evalSuite: string;
  evalProfile: string;
}

export const blankJobForm = (): JobForm => ({
  method: "sft", trainer: "", base: "", dataset: "", name: "", iters: "", epochs: "", rank: "", lr: "", batch: "", evalSuite: "", evalProfile: "",
});

function optionalNumber(text: string, label: string, check: (n: number) => boolean, rule: string): { value?: number; error?: string } {
  const t = text.trim().replace(",", ".");
  if (!t) return {};
  const n = Number(t);
  return Number.isFinite(n) && check(n) ? { value: n } : { error: `${label}: ${rule}.` };
}

/** The request for the form, or the first thing wrong with it. The engine checks the same ranges again. */
export function jobRequest(f: JobForm): { req: TrainJobRequest; error?: undefined } | { req?: undefined; error: string } {
  if (!f.base.trim()) return { error: "Indica il modello base." };
  if (!f.dataset) return { error: "Scegli un dataset." };
  if (f.evalSuite && !f.evalProfile) return { error: "Per confrontare l'adapter serve anche il profilo da cui partire." };
  const fields = [
    optionalNumber(f.iters, "Passi", (n) => Number.isInteger(n) && n >= 1 && n <= 1_000_000, "un intero da 1 a 1.000.000"),
    optionalNumber(f.epochs, "Epoche", (n) => n > 0 && n <= 100, "un numero tra 0 (escluso) e 100"),
    optionalNumber(f.rank, "Rango", (n) => Number.isInteger(n) && n >= 1 && n <= 256, "un intero da 1 a 256"),
    optionalNumber(f.lr, "Learning rate", (n) => n > 0 && n < 1, "un numero tra 0 e 1 (esclusi)"),
    optionalNumber(f.batch, "Batch", (n) => Number.isInteger(n) && n >= 1 && n <= 1024, "un intero da 1 a 1024"),
  ];
  const bad = fields.find((x) => x.error);
  if (bad) return { error: bad.error! };
  const [iters, epochs, rank, lr, batch] = fields.map((x) => x.value);
  const params: TrainJobRequest["params"] = {};
  if (iters !== undefined) params.iters = iters;
  if (epochs !== undefined) params.epochs = epochs;
  if (rank !== undefined) params.rank = rank;
  if (lr !== undefined) params.learning_rate = lr;
  if (batch !== undefined) params.batch_size = batch;
  const req: TrainJobRequest = { method: f.method, base_model: f.base.trim(), dataset_id: f.dataset, params };
  if (f.trainer) req.trainer = f.trainer;
  if (f.name.trim()) req.name = f.name.trim();
  if (f.evalSuite) {
    req.eval_suite = f.evalSuite;
    req.eval_profile = f.evalProfile;
  }
  return { req };
}

/** The runtime a profile can use an adapter with, or `null` when none of them loads this format yet. */
export function adapterRuntime(a: Pick<AdapterInfo, "format">): string | null {
  return a.format === "mlx" ? "mlx-lm" : a.format === "gguf" ? "llama-server" : null;
}
