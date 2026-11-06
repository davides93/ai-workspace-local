import { describe, expect, it } from "vitest";
import type { TrainDoctor, TrainerInfo } from "../../../shared/api/types";
import { blankDatasetForm, blankJobForm, datasetSpec, METHOD_SOURCES, METHODS, sourceLabel, isActive, jobPercent, jobRequest, methodBlocker, metricsLine, metricText, readyTrainers, adapterRuntime } from "./training";

const trainer = (id: string, state: TrainerInfo["state"], methods: TrainerInfo["methods"], reason?: string): TrainerInfo => ({
  id, name: id.toUpperCase(), description: "", state, reason, methods, adapter_format: "peft", install_auto: true,
});
const doctor = (trainers: TrainerInfo[]): TrainDoctor => ({
  device: { name: "d", os: "linux", arch: "x86_64", tier: "m", memory_mb: 1, vram_mb: 0, unified_memory: false, accelerators: [] },
  trainers, can_train: [], remote: [], advice: [],
});

describe("trainers of the device", () => {
  const d = doctor([
    trainer("a", "ready", [{ method: "sft", max_params_b: 4 }, { method: "dpo", blocked: "poca memoria" }]),
    trainer("b", "missing", [{ method: "sft", max_params_b: 8 }, { method: "dpo", max_params_b: 4 }]),
    trainer("c", "unsupported", [{ method: "sft", blocked: "serve un Mac" }], "disponibile su: macos"),
  ]);
  it("lists only the ready ones that can do the method", () => {
    expect(readyTrainers(d, "sft").map((t) => t.id)).toEqual(["a"]);
    expect(readyTrainers(d, "dpo")).toEqual([]);
    expect(readyTrainers(undefined, "sft")).toEqual([]);
  });
  it("says why a method cannot start", () => {
    expect(methodBlocker(d, "sft")).toBeNull();
    expect(methodBlocker(d, "dpo")).toBe("B non è installato.");
    expect(methodBlocker(doctor([trainer("c", "unsupported", [{ method: "sft", blocked: "serve un Mac" }])]), "sft")).toBe("serve un Mac");
    expect(methodBlocker(d, "grpo")).toMatch(/Nessun trainer/);
    expect(methodBlocker(undefined, "sft")).toBeNull();
  });
});

describe("job state", () => {
  it("polls only queued and running jobs", () => {
    expect(["queued", "running"].map((status) => isActive({ status: status as "queued" }))).toEqual([true, true]);
    expect(["succeeded", "failed", "cancelled"].map((status) => isActive({ status: status as "failed" }))).toEqual([false, false, false]);
  });
  it("shows the progress the trainer printed, clamped", () => {
    expect(jobPercent({ status: "running" })).toBeNull();
    expect(jobPercent({ status: "running", progress: 0.456 })).toBe(46);
    expect(jobPercent({ status: "running", progress: 3 })).toBe(100);
    expect(jobPercent({ status: "succeeded" })).toBe(100);
  });
  it("sums up the metrics in words", () => {
    expect(metricsLine({ train_loss: 0.84, val_loss: 0.97, steps: 51, duration_s: 840.4, eval: {} })).toBe("loss 0,84 · loss di validazione 0,97 · 51 passi · 840 s");
    expect(metricsLine(undefined)).toBe("");
    expect(metricText(3)).toBe("3");
  });
  it("knows which runtime loads an adapter", () => {
    expect(adapterRuntime({ format: "mlx" })).toBe("mlx-lm");
    expect(adapterRuntime({ format: "gguf" })).toBe("llama-server");
    expect(adapterRuntime({ format: "peft" })).toBeNull();
  });
});

describe("dataset form", () => {
  it("builds the request from what was filled in", () => {
    expect(datasetSpec(blankDatasetForm()).spec).toEqual({ method: "sft", sources: ["feedback"] });
    const f = { ...blankDatasetForm(), name: " Mio ", agent: "operator", since: "2026-01-01", limit: "50", sources: ["feedback", "eval"], evalReports: "evr_1, evr_2" };
    expect(datasetSpec(f).spec).toEqual({ method: "sft", name: "Mio", agent: "operator", since: "2026-01-01", limit: 50, sources: ["feedback", "eval"], eval_reports: ["evr_1", "evr_2"] });
  });
  it("offers DPO the sources a pair can come from", () => {
    expect(METHODS).toContain("dpo");
    expect(METHOD_SOURCES.dpo).toEqual(["feedback", "eval"]);
    expect(datasetSpec(blankDatasetForm("dpo")).spec).toEqual({ method: "dpo", sources: ["feedback"] });
  });
  it("asks GRPO for a suite and leaves the sources out", () => {
    expect(METHODS).toContain("grpo");
    expect(datasetSpec(blankDatasetForm("grpo")).error).toMatch(/suite/);
    const f = { ...blankDatasetForm("grpo"), suite: "esempio", repeat: "2" };
    expect(datasetSpec(f).spec).toEqual({ method: "grpo", suite: "esempio", repeat: 2 });
    expect(datasetSpec({ ...f, repeat: "0" }).error).toMatch(/ripetizioni/);
    expect(sourceLabel("suite:esempio")).toBe("Suite esempio");
  });
  it("catches the mistakes the engine would refuse", () => {
    expect(datasetSpec({ ...blankDatasetForm(), limit: "0" }).error).toMatch(/limite/);
    expect(datasetSpec({ ...blankDatasetForm(), limit: "2,5" }).error).toMatch(/limite/);
    expect(datasetSpec({ ...blankDatasetForm(), sources: [] }).error).toMatch(/sorgente/);
    expect(datasetSpec({ ...blankDatasetForm(), sources: ["eval"] }).error).toMatch(/report/);
  });
});

describe("job form", () => {
  const ok = { ...blankJobForm(), base: " Qwen/Qwen3-4B ", dataset: "ds_1" };
  it("builds the request, leaving out what is empty", () => {
    expect(jobRequest(ok).req).toEqual({ method: "sft", base_model: "Qwen/Qwen3-4B", dataset_id: "ds_1", params: {} });
    const f = { ...ok, trainer: "trl", name: "Mio", iters: "100", epochs: "2,5", rank: "8", lr: "2e-4", batch: "4", evalSuite: "esempio", evalProfile: "default" };
    expect(jobRequest(f).req).toEqual({
      method: "sft", trainer: "trl", base_model: "Qwen/Qwen3-4B", dataset_id: "ds_1", name: "Mio", eval_suite: "esempio", eval_profile: "default",
      params: { iters: 100, epochs: 2.5, rank: 8, learning_rate: 2e-4, batch_size: 4 },
    });
  });
  it("catches what the engine would refuse", () => {
    expect(jobRequest({ ...ok, base: " " }).error).toMatch(/modello base/);
    expect(jobRequest({ ...ok, dataset: "" }).error).toMatch(/dataset/);
    expect(jobRequest({ ...ok, rank: "0" }).error).toMatch(/Rango/);
    expect(jobRequest({ ...ok, lr: "1" }).error).toMatch(/Learning rate/);
    expect(jobRequest({ ...ok, iters: "1,5" }).error).toMatch(/Passi/);
    expect(jobRequest({ ...ok, epochs: "abc" }).error).toMatch(/Epoche/);
    expect(jobRequest({ ...ok, evalSuite: "esempio" }).error).toMatch(/profilo/);
  });
});
