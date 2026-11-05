import { describe, expect, it } from "vitest";
import type { PromptOptimization } from "../../../shared/api/types";
import { canApply, defaultVariant, diffLineKind, isOptimizing, optRows, variantsError, verdict } from "./optimizations";

const opt = (over: Partial<PromptOptimization> = {}): PromptOptimization => ({
  id: "opt_1", suite_id: "s", agent_id: "operator", status: "done", started_at: "t", base_prompt: "a",
  baseline_score: 0.5, baseline_pass_rate: 0.5,
  variants: [
    { prompt: "b", rationale: "r1", score: 0.4, pass_rate: 0.5, report_id: "evr_1", diff: "" },
    { prompt: "c", rationale: "r2", score: 0.9, pass_rate: 1, report_id: "evr_2", diff: "" },
  ],
  ...over,
});

describe("variantsError", () => {
  it("accepts 1 to 5 whole variants", () => {
    for (const ok of ["1", "3", "5"]) expect(variantsError(ok)).toBeUndefined();
    for (const bad of ["0", "6", "2.5", "", "abc", "-1"]) expect(variantsError(bad)).toMatch(/1 a 5/);
  });
});

describe("optRows", () => {
  it("lists the current prompt, then the variants with their gap from it", () => {
    const rows = optRows(opt({ best: 1 }));
    expect(rows.map((r) => [r.key, r.label, r.best])).toEqual([["base", "Prompt attuale", false], [0, "Variante 1", false], [1, "Variante 2", true]]);
    expect(rows[0].delta).toBeUndefined();
    expect(rows[1].delta).toBeCloseTo(-0.1);
    expect(rows[2].delta).toBeCloseTo(0.4);
    expect(rows[2].reportId).toBe("evr_2");
  });
  it("marks the applied variant", () => {
    expect(optRows(opt({ best: 1, applied: 1, status: "applied" })).map((r) => r.applied)).toEqual([false, false, true]);
  });
});

describe("what to show and offer", () => {
  it("opens the applied variant, else the best, else the first", () => {
    expect(defaultVariant(opt({ best: 1 }))).toBe(1);
    expect(defaultVariant(opt({ best: 1, applied: 0 }))).toBe(0);
    expect(defaultVariant(opt())).toBe(0);
    expect(defaultVariant(opt({ variants: [] }))).toBeNull();
  });
  it("proposes to apply only the best variant of a finished optimization", () => {
    expect(canApply(opt({ best: 1 }), 1)).toBe(true);
    expect(canApply(opt({ best: 1 }), 0)).toBe(false);
    expect(canApply(opt(), 0)).toBe(false);
    expect(canApply(opt({ best: 1, status: "applied", applied: 1 }), 1)).toBe(false);
    expect(canApply(opt({ best: 1, status: "running" }), 1)).toBe(false);
  });
  it("says so when nothing beats the current prompt", () => {
    expect(verdict(opt())).toMatch(/nulla da applicare/);
    expect(verdict(opt({ best: 1 }))).toMatch(/variante 2 batte/);
    expect(verdict(opt({ status: "applied", applied: 1 }))).toMatch(/variante 2/);
    expect(verdict(opt({ status: "failed", error: "boom" }))).toBe("boom");
    expect(verdict(opt({ status: "running" }))).toMatch(/In corso/);
    expect(isOptimizing(opt({ status: "running" }))).toBe(true);
    expect(isOptimizing(opt())).toBe(false);
  });
});

describe("diffLineKind", () => {
  it("tells added, removed, hunk and header lines", () => {
    expect(["--- attuale", "+++ variante", "@@ -1,2 +1,3 @@", "+nuova", "-vecchia", " uguale", "--- una riga che inizia con trattini"].map(diffLineKind)).toEqual(["head", "head", "hunk", "add", "del", "ctx", "del"]);
  });
});
