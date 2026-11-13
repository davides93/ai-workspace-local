import { labelMap, t as tr } from "../../../shared/i18n";
import type { AdapterInfo, DatasetSpec, TrainDoctor, TrainJob, TrainJobRequest, TrainMethod, TrainerInfo } from "../../../shared/api/types";

/** Poll interval of jobs and log, used only while a job is going. */
export const TRAIN_POLL_MS = 2_000;

/** Fewest examples (pairs for DPO) a dataset needs; the engine refuses fewer. */
export const MIN_EXAMPLES = 10;

export const METHOD_LABEL: Record<TrainMethod, string> = { sft: "SFT", dpo: "DPO", grpo: "GRPO" };

/** The methods the page offers. */
export const METHODS: TrainMethod[] = ["sft", "dpo", "grpo"];

/** Where the examples of each method can come from (GRPO takes its prompts from a suite instead). */
export const METHOD_SOURCES: Record<TrainMethod, string[]> = {
  sft: ["feedback", "succeeded", "eval"],
  dpo: ["feedback", "eval"],
  grpo: [],
};

export const METHOD_HINT: Record<TrainMethod, string> = labelMap({ sft: "training.hint_sft", dpo: "training.hint_dpo", grpo: "training.hint_grpo" });

export const SOURCE_LABEL: Record<string, string> = labelMap({
  import: "training.source_import",
  feedback: "training.source_feedback",
  succeeded: "training.source_succeeded",
  eval: "training.source_eval",
});

/** A source as the list shows it: `suite:<id>` is the suite a GRPO dataset takes its prompts from. */
export const sourceLabel = (s: string): string => (s.startsWith("suite:") ? tr("training.source_suite", { id: s.slice(6) }) : SOURCE_LABEL[s] ?? s);

/** What a source means for DPO: a pair needs both sides. */
export const DPO_SOURCE_LABEL: Record<string, string> = labelMap({ feedback: "training.dpo_feedback", eval: "training.dpo_eval" });

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
  if (knowing.length === 0) return tr("training.no_trainer_knows", { method: METHOD_LABEL[method] });
  const missing = knowing.find((t) => t.state === "missing" && t.methods.some((m) => m.method === method && !m.blocked));
  if (missing) return tr("training.not_installed", { name: missing.name });
  const why = knowing.map((t) => t.methods.find((m) => m.method === method)?.blocked ?? t.reason).find(Boolean);
  return why ?? tr("training.device_cannot", { method: METHOD_LABEL[method] });
}

/** `0.84` → "0,84"; text and other values are shown as they are. */
export function metricText(v: unknown): string {
  return typeof v === "number" ? (Number.isInteger(v) ? String(v) : v.toFixed(2).replace(".", ",")) : String(v);
}

const METRIC_LABEL: Record<string, string> = labelMap({ train_loss: "training.metric_train_loss", val_loss: "training.metric_val_loss", steps: "training.metric_steps", duration_s: "training.metric_seconds" });

/** "loss 0,84 · loss di validazione 0,97 · 51 passi" for the metrics a job or adapter reports. */
export function metricsLine(metrics: Record<string, unknown> | undefined): string {
  if (!metrics) return "";
  const parts: string[] = [];
  for (const k of ["train_loss", "val_loss"]) if (typeof metrics[k] === "number") parts.push(`${METRIC_LABEL[k]} ${metricText(metrics[k])}`);
  if (typeof metrics.steps === "number") parts.push(tr("training.n_steps", { n: metrics.steps }));
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
  /** GRPO: the suite whose tasks are the prompts and whose graders give the rewards. */
  suite: string;
  repeat: string;
}

export const blankDatasetForm = (method: TrainMethod = "sft"): DatasetForm => ({
  name: "", method, agent: "", sources: METHOD_SOURCES[method].length ? ["feedback"] : [], since: "", until: "", limit: "", evalReports: "", suite: "", repeat: "",
});

/** The request for the form, or the first thing wrong with it. */
export function datasetSpec(f: DatasetForm): { spec: DatasetSpec; error?: undefined } | { spec?: undefined; error: string } {
  const limit = f.limit.trim();
  if (limit && !(Number.isInteger(Number(limit)) && Number(limit) >= 1)) return { error: tr("training.limit_error") };
  if (METHOD_SOURCES[f.method].length > 0 && f.sources.length === 0) return { error: tr("training.pick_source") };
  const repeat = f.repeat.trim();
  if (f.method === "grpo") {
    if (!f.suite) return { error: tr("training.pick_suite") };
    if (repeat && !(Number.isInteger(Number(repeat)) && Number(repeat) >= 1)) return { error: tr("training.repeat_error") };
  }
  const reports = f.evalReports.split(/[\s,]+/).filter(Boolean);
  if (f.sources.includes("eval") && reports.length === 0) return { error: tr("training.eval_needs_report") };
  const spec: DatasetSpec = { method: f.method };
  if (f.name.trim()) spec.name = f.name.trim();
  if (f.agent) spec.agent = f.agent;
  if (f.since.trim()) spec.since = f.since.trim();
  if (f.until.trim()) spec.until = f.until.trim();
  if (limit) spec.limit = Number(limit);
  if (METHOD_SOURCES[f.method].length > 0) spec.sources = f.sources;
  if (f.method === "grpo") {
    spec.suite = f.suite;
    if (repeat) spec.repeat = Number(repeat);
    return { spec };
  }
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
  return Number.isFinite(n) && check(n) ? { value: n } : { error: tr("training.field_error", { label, rule }) };
}

/** The request for the form, or the first thing wrong with it. The engine checks the same ranges again. */
export function jobRequest(f: JobForm): { req: TrainJobRequest; error?: undefined } | { req?: undefined; error: string } {
  if (!f.base.trim()) return { error: tr("training.need_base") };
  if (!f.dataset) return { error: tr("training.need_dataset") };
  if (f.evalSuite && !f.evalProfile) return { error: tr("training.need_profile") };
  const fields = [
    optionalNumber(f.iters, tr("training.passi"), (n) => Number.isInteger(n) && n >= 1 && n <= 1_000_000, tr("training.rule_int_1_1m")),
    optionalNumber(f.epochs, tr("training.epoche"), (n) => n > 0 && n <= 100, tr("training.rule_epochs")),
    optionalNumber(f.rank, tr("training.rank"), (n) => Number.isInteger(n) && n >= 1 && n <= 256, tr("training.rule_int_1_256")),
    optionalNumber(f.lr, tr("training.learning_rate"), (n) => n > 0 && n < 1, tr("training.rule_lr")),
    optionalNumber(f.batch, tr("training.batch"), (n) => Number.isInteger(n) && n >= 1 && n <= 1024, tr("training.rule_int_1_1024")),
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
