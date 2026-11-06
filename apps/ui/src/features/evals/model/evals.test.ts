import { describe, expect, it } from "vitest";
import type { EvalReport, EvalTaskResult } from "../../../shared/api/types";
import { blankSuite, comparableWith, deltaTone, parseSuiteText, pct, pointsDelta, progress, reportWho, scoreDelta, suiteText, taskRows } from "./evals";

const task = (task_id: string, passed: boolean, score = passed ? 1 : 0, tokens = 100): EvalTaskResult => ({
  task_id, attempt: 1, passed, score, graders: [], duration_ms: 2000, tokens, turns: 2,
});
const report = (id: string, tasks: EvalTaskResult[], extra: Partial<EvalReport> = {}): EvalReport => ({
  id, suite_id: "s", suite_name: "S", status: "completed", started_at: "2026-01-01T00:00:00Z", repeat: 1, tasks, summary: { passed: 0, total: 0, score: 0, pass_rate: 0 }, ...extra,
});

describe("suite text", () => {
  it("round-trips a suite through the editor text", () => {
    const s = blankSuite("mia");
    const parsed = parseSuiteText(suiteText(s));
    expect(parsed.suite).toEqual(s);
  });
  it("explains what is wrong with the text before asking the server", () => {
    expect(parseSuiteText("{ non json").error).toMatch(/JSON non valido/);
    expect(parseSuiteText("[1]").error).toMatch(/oggetto/);
    expect(parseSuiteText('{"name": "x"}').error).toMatch(/id/);
    expect(parseSuiteText('{"id": "  "}').error).toMatch(/id/);
    // The rest is for the server to judge.
    expect(parseSuiteText('{"id": "x"}').suite?.id).toBe("x");
  });
});

describe("task rows", () => {
  it("averages the attempts of each task, in the order they ran", () => {
    const rows = taskRows(report("r", [task("b", true, 1, 100), task("a", false, 0, 300), task("b", false, 0.5, 300)]));
    expect(rows.map((r) => r.task_id)).toEqual(["b", "a"]);
    expect(rows[0]).toMatchObject({ attempts: 2, passed: 1, pass_rate: 0.5, score: 0.75, avg_tokens: 200 });
    expect(rows[1]).toMatchObject({ attempts: 1, passed: 0, pass_rate: 0 });
    expect(taskRows(report("r", []))).toEqual([]);
  });
});

describe("progress and labels", () => {
  it("counts attempts done out of tasks × repeat", () => {
    const r = report("r", [task("a", true)], { repeat: 3 });
    expect(progress(r, { tasks: [{ id: "a", prompt: "", graders: [] }, { id: "b", prompt: "", graders: [] }] })).toEqual({ done: 1, total: 6 });
    expect(progress(r, undefined)).toEqual({ done: 1, total: null });
  });
  it("names who made a report", () => {
    expect(reportWho(report("r", [], { agent: "coder", profile: "veloce" }))).toBe("coder · veloce");
    expect(reportWho(report("r", [], { profile: "veloce" }))).toBe("veloce");
    expect(reportWho(report("r", []))).toBe("predefiniti della suite");
  });
  it("shows ratios as percentages", () => {
    expect(pct(0.756)).toBe("76%");
    expect(pct(0)).toBe("0%");
  });
});

describe("deltas", () => {
  it("signs the difference in points and in score", () => {
    expect(pointsDelta(0.25)).toBe("+25 pt");
    expect(pointsDelta(-0.1)).toBe("−10 pt");
    expect(pointsDelta(0)).toBe("0 pt");
    expect(pointsDelta(null)).toBe("—");
    expect(scoreDelta(0.5)).toBe("+0,50");
    expect(scoreDelta(-0.123)).toBe("−0,12");
    expect(scoreDelta(0)).toBe("0,00");
    expect(scoreDelta(undefined)).toBe("—");
  });
  it("tells better from worse, ignoring noise", () => {
    expect(deltaTone(0.3)).toBe("better");
    expect(deltaTone(-0.3)).toBe("worse");
    expect(deltaTone(0.001)).toBe("same");
    expect(deltaTone(null)).toBeNull();
  });
});

describe("what a report can be compared with", () => {
  it("offers other reports of the same suite that ran something", () => {
    const a = report("a", [task("t", true)]);
    const all = [a, report("b", [task("t", false)]), report("c", [], { status: "running" }), report("d", [task("t", true)], { suite_id: "altra" })];
    expect(comparableWith(a, all).map((r) => r.id)).toEqual(["b"]);
  });
});
