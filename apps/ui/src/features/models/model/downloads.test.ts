import { describe, expect, it } from "vitest";
import { DOWNLOAD_STATUS, etaSeconds, filesLabel, formatEta, isActiveJob, jobName, progressRatio, retryPlan, sortJobs, summarizeQueue } from "./downloads";
import { job } from "./testdata";

const MB = 1024 ** 2;

describe("isActiveJob", () => {
  it("is true only while waiting, transferring or verifying", () => {
    expect((["queued", "running", "verifying"] as const).every((status) => isActiveJob({ status }))).toBe(true);
    expect((["done", "error", "cancelled"] as const).some((status) => isActiveJob({ status }))).toBe(false);
  });
  it("labels every status the backend can send", () => {
    expect(Object.keys(DOWNLOAD_STATUS).sort()).toEqual(["cancelled", "done", "error", "queued", "running", "verifying"]);
  });
});

describe("progress and ETA", () => {
  it("clamps the ratio and treats a done job as complete", () => {
    expect(progressRatio(job({ received: 250, total: 1000 }))).toBe(0.25);
    expect(progressRatio(job({ received: 2000, total: 1000 }))).toBe(1);
    expect(progressRatio(job({ received: 0, total: 0 }))).toBe(0);
    expect(progressRatio(job({ received: 10, total: 0, status: "done" }))).toBe(1);
  });

  it("divides what is left by the speed", () => {
    expect(etaSeconds(job({ received: 400 * MB, total: 1000 * MB, speed_bps: 10 * MB }))).toBe(60);
  });

  it("gives no ETA when it cannot be known", () => {
    expect(etaSeconds(job({ speed_bps: 0 }))).toBeNull();
    expect(etaSeconds(job({ received: 1000, total: 1000, speed_bps: 5 }))).toBeNull();
    expect(etaSeconds(job({ status: "queued", speed_bps: 5 }))).toBeNull();
    expect(etaSeconds(job({ status: "verifying", speed_bps: 5 }))).toBeNull();
    expect(etaSeconds(job({ status: "error", speed_bps: 5 }))).toBeNull();
  });

  it("words the ETA in Italian", () => {
    expect(formatEta(null)).toBe("—");
    expect(formatEta(4)).toBe("pochi secondi");
    expect(formatEta(35)).toBe("35 s");
    expect(formatEta(240)).toBe("circa 4 min");
    expect(formatEta(4900)).toBe("circa 1 h 21 min");
  });

  it("describes the file being transferred", () => {
    expect(filesLabel(job({ files_total: 1 }))).toBeNull();
    expect(filesLabel(job({ files_total: 3, files_done: 1, status: "running" }))).toBe("file 2 di 3");
    expect(filesLabel(job({ files_total: 3, files_done: 3, status: "running" }))).toBe("file 3 di 3");
    expect(filesLabel(job({ files_total: 3, files_done: 3, status: "done" }))).toBe("3 file");
  });

  it("names a job by its reference, falling back to the id and the file", () => {
    expect(jobName(job({ model_ref: "a/b" }))).toBe("a/b");
    expect(jobName(job({ model_ref: "", model_id: "qwen3-4b" }))).toBe("qwen3-4b");
    expect(jobName(job({ model_ref: "", model_id: "", file: "x.gguf" }))).toBe("x.gguf");
  });
});

describe("summarizeQueue and sortJobs", () => {
  const jobs = [
    job({ id: "done", status: "done", received: 10, total: 10 }),
    job({ id: "q", status: "queued", received: 0, total: 500 }),
    job({ id: "r1", status: "running", received: 100, total: 600, speed_bps: 50 }),
    job({ id: "err", status: "error" }),
    job({ id: "r2", status: "running", received: 0, total: 400, speed_bps: 50 }),
    job({ id: "v", status: "verifying", received: 300, total: 300 }),
    job({ id: "can", status: "cancelled" }),
  ];

  it("counts what is active, failed and finished, and sums speed and what is left", () => {
    const s = summarizeQueue(jobs);
    expect(s).toMatchObject({ active: 4, running: 2, queued: 1, verifying: 1, failed: 1, finished: 2, speedBps: 100 });
    expect(s.remainingBytes).toBe(500 + 500 + 400 + 0);
    expect(s.etaSeconds).toBe(14);
  });

  it("has no ETA with nothing running", () => {
    expect(summarizeQueue([job({ status: "queued" })]).etaSeconds).toBeNull();
    expect(summarizeQueue([]).active).toBe(0);
  });

  it("shows transferring jobs first, then waiting, failed and finished, keeping the server order inside a group", () => {
    expect(sortJobs(jobs).map((j) => j.id)).toEqual(["r1", "r2", "v", "q", "err", "done", "can"]);
  });
});

describe("retryPlan", () => {
  const catalog = new Set(["qwen3-4b"]);

  it("pulls a curated catalog entry again by its id", () => {
    expect(retryPlan(job({ status: "error", model_id: "qwen3-4b", provider: "hf" }), catalog)).toEqual({ kind: "pull", request: "qwen3-4b" });
  });

  it("rebuilds a hub request from provider, model_ref and variant", () => {
    expect(retryPlan(job({ status: "error", provider: "hf", model_ref: "Qwen/Qwen3-4B-GGUF", variant: "Q8_0" }), catalog)).toEqual({
      kind: "pull",
      request: { provider: "hf", model: "Qwen/Qwen3-4B-GGUF", variant: "Q8_0" },
    });
    expect(retryPlan(job({ status: "cancelled", provider: "ollama", model_ref: "qwen3:4b", variant: "" }), catalog)).toEqual({
      kind: "pull",
      request: { provider: "ollama", model: "qwen3:4b", variant: "" },
    });
  });

  it("reopens the URL form instead of guessing the optional name and sha256", () => {
    expect(retryPlan(job({ status: "error", provider: "url", model_ref: "https://x.org/a.gguf", model_id: "url/a" }), catalog)).toEqual({ kind: "url", url: "https://x.org/a.gguf" });
    expect(retryPlan(job({ status: "error", provider: "url", model_ref: "not-a-url", model_id: "url/a" }), catalog)).toBeNull();
  });

  it("offers nothing for active or finished jobs, or when the request cannot be rebuilt", () => {
    expect(retryPlan(job({ status: "running" }), catalog)).toBeNull();
    expect(retryPlan(job({ status: "done" }), catalog)).toBeNull();
    expect(retryPlan(job({ status: "error", provider: "", model_ref: "", model_id: "legacy" }), catalog)).toBeNull();
    expect(retryPlan(job({ status: "error", provider: "hf", model_ref: "" }), catalog)).toBeNull();
  });
});
