import { describe, expect, it } from "vitest";
import { downloadsInterval, isActiveDownload } from "../../features/models/api";
import { isActiveRun } from "../../features/workflows/api";
import type { DownloadProgress } from "../api/types";
import { qk } from "./keys";

const startsWith = (key: readonly unknown[], prefix: readonly unknown[]) => prefix.every((p, i) => JSON.stringify(key[i]) === JSON.stringify(p));

describe("query keys", () => {
  it("are stable for the same input", () => {
    expect(qk.models.search({ provider: "hf", q: "qwen" })).toEqual(qk.models.search({ provider: "hf", q: "qwen" }));
    expect(qk.sessions.messages("a")).toEqual(qk.sessions.messages("a"));
  });
  it("differ when the input differs", () => {
    expect(JSON.stringify(qk.models.search({ provider: "hf", q: "qwen" }))).not.toBe(JSON.stringify(qk.models.search({ provider: "hf", q: "gemma" })));
    expect(qk.sessions.messages("a")).not.toEqual(qk.sessions.messages("b"));
    expect(qk.models.remote("hf", "x/y")).not.toEqual(qk.models.remote("hf", "x/y", "v2"));
  });
  it("nest under their domain so a domain can be invalidated at once", () => {
    for (const k of [qk.models.installed(), qk.models.catalog(), qk.models.providers(), qk.models.downloads(), qk.models.remote("hf", "a/b"), qk.models.search({ provider: "hf", q: "" })]) {
      expect(startsWith(k, qk.models.all())).toBe(true);
    }
    for (const k of [qk.runtimes.list(), qk.runtimes.logs("llama-server")]) expect(startsWith(k, qk.runtimes.all())).toBe(true);
    for (const k of [qk.sessions.list(), qk.sessions.detail("a"), qk.sessions.messages("a"), qk.sessions.worktree("a")]) expect(startsWith(k, qk.sessions.all())).toBe(true);
    for (const k of [qk.workflows.list(), qk.workflows.runs()]) expect(startsWith(k, qk.workflows.all())).toBe(true);
  });
  it("keeps different domains apart", () => {
    expect(qk.models.installed()).not.toEqual(qk.runtimes.list());
    expect(qk.policy()).not.toEqual(qk.hooks());
  });
});

const job = (status: DownloadProgress["status"]): DownloadProgress => ({
  id: status, model_id: "m", file: "f", received: 0, total: 1, status, provider: "hf", model_ref: "a/b", variant: "Q4_K_M", files_total: 1, files_done: 0, speed_bps: 0, dest: "",
});

describe("polling rules", () => {
  it("polls downloads only while a job is queued, running or verifying", () => {
    expect(downloadsInterval(undefined)).toBe(false);
    expect(downloadsInterval([])).toBe(false);
    expect(downloadsInterval([job("done"), job("error"), job("cancelled")])).toBe(false);
    expect(downloadsInterval([job("done"), job("queued")])).toBe(1000);
    expect(downloadsInterval([job("running")])).toBe(1000);
    expect(downloadsInterval([job("verifying")])).toBe(1000);
  });
  it("classifies active jobs and workflow runs", () => {
    expect(isActiveDownload(job("verifying"))).toBe(true);
    expect(isActiveDownload(job("error"))).toBe(false);
    expect(isActiveRun({ status: "running" })).toBe(true);
    expect(isActiveRun({ status: "waiting_approval" })).toBe(true);
    expect(isActiveRun({ status: "succeeded" })).toBe(false);
  });
});
