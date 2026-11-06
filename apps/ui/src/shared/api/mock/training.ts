import type { AdapterInfo, DatasetInfo, DatasetSpec, TrainDoctor, TrainJob, TrainJobRequest, TrainerInfo } from "../types";
import { ApiError } from "../http";
import { nid, now, sleep } from "./fixtures";

// Demo data and rules of the fine-tuning API, as the engine applies them (crates/aiw-engine/src/{dataset,train_jobs}.rs).

const iso = (msAgo: number) => new Date(Date.now() - msAgo).toISOString();

/** Fewest examples (or pairs) a dataset needs. */
export const MIN_EXAMPLES = 10;

export const trainers: TrainerInfo[] = [
  {
    id: "mlx-lm-lora", name: "MLX LM (LoRA)", description: "LoRA SFT su Apple Silicon, con i modelli MLX.", state: "unsupported",
    reason: "disponibile su: macos", adapter_format: "mlx", install_auto: true,
    methods: [{ method: "sft", blocked: "serve un Mac con Apple Silicon" }],
  },
  {
    id: "trl", name: "TRL + PEFT", description: "SFT, DPO e GRPO con Hugging Face TRL; serve una GPU NVIDIA.", state: "ready",
    adapter_format: "peft", install_auto: true,
    methods: [
      { method: "sft", max_params_b: 14 },
      { method: "dpo", max_params_b: 9 },
      { method: "grpo", blocked: "GRPO richiede un dispositivo di classe L o superiore con TRL: qui è XL, ma la memoria della GPU non basta" },
    ],
  },
  {
    id: "unsloth", name: "Unsloth", description: "LoRA più veloce e leggero su GPU NVIDIA.", state: "missing", adapter_format: "peft", install_auto: true,
    methods: [{ method: "sft", max_params_b: 20 }],
  },
];

export const doctor = (): TrainDoctor => ({
  device: { name: "Workstation demo", os: "windows", arch: "x86_64", tier: "xl", memory_mb: 65536, vram_mb: 24564, unified_memory: false, accelerators: ["cuda", "cpu"] },
  trainers: structuredClone(trainers),
  can_train: ["LoRA SFT fino a ~14B su questa GPU (TRL + PEFT)", "DPO fino a ~9B su questa GPU (TRL + PEFT)"],
  remote: ["MacBook Pro M4 Max (classe XL) può fare LoRA SFT con MLX LM (LoRA)"],
  advice: [],
});

export const datasets: DatasetInfo[] = [
  {
    id: "ds_demo1", name: "SFT approvate", method: "sft", created_at: iso(3 * 86_400_000), examples: 38, train: 34, valid: 4, tokens_est: 21_400,
    agents: ["operator"], sources: ["feedback"], path: "~/.aiw/datasets/ds_demo1",
  },
];

export const jobs: TrainJob[] = [
  {
    id: "trn_demo1", method: "sft", trainer: "trl", base_model: "Qwen/Qwen3-4B", dataset_id: "ds_demo1", params: { epochs: 3, rank: 16 }, status: "succeeded",
    created_at: iso(2 * 86_400_000), started_at: iso(2 * 86_400_000), finished_at: iso(2 * 86_400_000 - 14 * 60_000), log_path: "~/.aiw/train/trn_demo1/train.log",
    progress: 1, adapter_id: "ad_demo1", metrics: { train_loss: 0.84, val_loss: 0.97, steps: 51, duration_s: 840 }, name: "Operator v1",
  },
];

export const adapters: AdapterInfo[] = [
  {
    id: "ad_demo1", name: "Operator v1", base_model: "Qwen/Qwen3-4B", method: "sft", trainer: "trl", dataset_id: "ds_demo1", created_at: iso(2 * 86_400_000 - 14 * 60_000),
    path: "~/.aiw/adapters/ad_demo1", metrics: { train_loss: 0.84, val_loss: 0.97, steps: 51 }, format: "peft", job_id: "trn_demo1",
  },
];

const logs = new Map<string, string[]>([["trn_demo1", ["Loading model Qwen/Qwen3-4B", "AIW_PROGRESS {\"step\": 51, \"total\": 51, \"loss\": 0.84}", "Addestramento completato."]]]);

export function buildDataset(spec: DatasetSpec): DatasetInfo {
  if (!["sft", "dpo", "grpo"].includes(spec.method)) throw new ApiError(400, `metodo «${spec.method}» sconosciuto: usa sft, dpo o grpo`);
  const sources = spec.sources?.length ? spec.sources : ["feedback"];
  const bad = sources.find((s) => !["feedback", "succeeded", "eval"].includes(s));
  if (bad) throw new ApiError(400, `sorgente «${bad}» sconosciuta: usa feedback, succeeded o eval`);
  if (sources.includes("eval") && !spec.eval_reports?.length) throw new ApiError(400, "la sorgente eval richiede i report di valutazione da cui prendere i task riusciti");
  // Demo content: more examples when more sources are asked for.
  let found = 6 + 14 * sources.length;
  if (spec.agent && spec.agent !== "operator") found = 3;
  if (spec.limit) found = Math.min(found, spec.limit);
  if (found < MIN_EXAMPLES) {
    const noun = spec.method === "dpo" ? "coppie" : "esempi";
    throw new ApiError(400, `servono almeno ${MIN_EXAMPLES} ${noun} per addestrare, ne ho trovati ${found}. Dai un pollice alle risposte buone o allarga i filtri.`);
  }
  const valid = Math.max(1, Math.round(found * 0.1));
  const id = nid("ds");
  const info: DatasetInfo = {
    id, name: spec.name?.trim() || `${spec.method.toUpperCase()} ${id.slice(3, 9)}`, method: spec.method, created_at: now(), examples: found, train: found - valid, valid,
    tokens_est: found * 560, agents: [spec.agent ?? "operator"], sources, path: `~/.aiw/datasets/${id}`,
  };
  datasets.unshift(info);
  return structuredClone(info);
}

export function deleteDataset(id: string): void {
  const i = datasets.findIndex((d) => d.id === id);
  if (i < 0) throw new ApiError(404, `dataset ${id} non trovato`);
  if (jobs.some((j) => j.dataset_id === id && (j.status === "queued" || j.status === "running"))) {
    throw new ApiError(409, "il dataset è in uso da un addestramento in corso");
  }
  datasets.splice(i, 1);
}

/** Cancel switches of the running demo jobs. */
const cancelled = new Set<string>();

export function startJob(req: TrainJobRequest): TrainJob {
  if (!["sft", "dpo", "grpo"].includes(req.method)) throw new ApiError(400, `metodo «${req.method}» sconosciuto: usa sft, dpo o grpo`);
  if (!req.base_model.trim()) throw new ApiError(400, "indica il modello base");
  const ds = datasets.find((d) => d.id === req.dataset_id);
  if (!ds) throw new ApiError(404, `dataset ${req.dataset_id} non trovato`);
  if (ds.method !== req.method) throw new ApiError(400, `il dataset «${ds.name}» è per ${ds.method.toUpperCase()}, non per ${req.method.toUpperCase()}`);
  const trainer = trainers.find((t) => t.id === (req.trainer ?? "trl"));
  if (!trainer) throw new ApiError(404, `trainer ${req.trainer} non trovato`);
  if (trainer.state !== "ready") throw new ApiError(422, trainer.state === "missing" ? `${trainer.name} non è installato: \`aiw train install ${trainer.id}\`` : `${trainer.name} non si può usare qui: ${trainer.reason ?? ""}`);
  const fit = trainer.methods.find((m) => m.method === req.method);
  if (!fit || fit.blocked) throw new ApiError(422, `${req.method} con ${trainer.name} non è possibile qui: ${fit?.blocked ?? "metodo non supportato"}`);
  if (jobs.some((j) => j.status === "queued" || j.status === "running")) throw new ApiError(409, "c'è già un addestramento in corso: aspetta che finisca o annullalo");
  const job: TrainJob = {
    id: nid("trn"), method: req.method, trainer: trainer.id, base_model: req.base_model.trim(), dataset_id: ds.id, params: req.params ?? {}, status: "queued",
    created_at: now(), log_path: "~/.aiw/train/job/train.log", ...(req.name?.trim() ? { name: req.name.trim() } : {}),
    ...(req.eval_suite ? { eval_suite: req.eval_suite } : {}), ...(req.eval_profile ? { eval_profile: req.eval_profile } : {}),
  };
  jobs.unshift(job);
  logs.set(job.id, []);
  void run(job, ds);
  return structuredClone(job);
}

async function run(job: TrainJob, ds: DatasetInfo) {
  const log = logs.get(job.id)!;
  await sleep(500);
  job.status = "running";
  job.started_at = now();
  const total = job.params.iters ?? Math.max(5, Math.ceil(((job.params.epochs ?? 3) * ds.train) / (job.params.batch_size ?? 4)));
  log.push(`Loading model ${job.base_model}`);
  for (let step = 1; step <= total; step++) {
    await sleep(350);
    if (cancelled.delete(job.id)) {
      job.status = "cancelled";
      job.finished_at = now();
      return;
    }
    const loss = 2.4 / (1 + step * 0.35);
    job.progress = step / total;
    log.push(`AIW_PROGRESS {"step": ${step}, "total": ${total}, "loss": ${loss.toFixed(3)}}`);
  }
  const id = nid("ad");
  const metrics = { train_loss: 0.71, val_loss: 0.88, steps: total, duration_s: Math.round(total * 0.35) };
  adapters.unshift({
    id, name: job.name ?? `${job.method.toUpperCase()} · ${job.base_model.split("/").pop()}`, base_model: job.base_model, method: job.method, trainer: job.trainer,
    dataset_id: job.dataset_id, created_at: now(), path: `~/.aiw/adapters/${id}`, metrics, format: trainers.find((t) => t.id === job.trainer)?.adapter_format ?? "peft", job_id: job.id,
  });
  log.push("Addestramento completato.");
  job.adapter_id = id;
  job.metrics = metrics;
  job.progress = 1;
  job.status = "succeeded";
  job.finished_at = now();
}

export function cancelJob(id: string): TrainJob {
  const job = jobs.find((j) => j.id === id);
  if (!job) throw new ApiError(404, `addestramento ${id} non trovato`);
  if (job.status !== "queued" && job.status !== "running") throw new ApiError(409, `l'addestramento ${id} non è in corso (${job.status})`);
  cancelled.add(id);
  return structuredClone(job);
}

export const jobLog = (id: string, lines: number): string[] => {
  if (!jobs.some((j) => j.id === id)) throw new ApiError(404, `addestramento ${id} non trovato`);
  return (logs.get(id) ?? []).slice(-lines);
};

/** The profiles that name the adapter's path keep it from being deleted (the mock has none). */
export function deleteAdapter(id: string): void {
  const i = adapters.findIndex((a) => a.id === id);
  if (i < 0) throw new ApiError(404, `adapter ${id} non trovato`);
  adapters.splice(i, 1);
}
