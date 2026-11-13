import type { PromptOptimization } from "../../../shared/api/types";

import { t } from "../../../shared/i18n";
/** The optimization is still going: the page polls it. */
export const isOptimizing = (o: Pick<PromptOptimization, "status">): boolean => o.status === "running";

/** Poll interval, used only while one of them is still going. */
export const OPT_POLL_MS = 2_000;

/** Variants the engine accepts, and the default. */
export const MIN_VARIANTS = 1;
export const MAX_VARIANTS = 5;
export const DEFAULT_VARIANTS = 3;

/** Why the number of variants is wrong, if it is. */
export function variantsError(text: string): string | undefined {
  const n = Number(text);
  return Number.isInteger(n) && n >= MIN_VARIANTS && n <= MAX_VARIANTS ? undefined : t("evals.integer_range", { min: MIN_VARIANTS, max: MAX_VARIANTS });
}

export interface OptRow {
  /** `base` for the current prompt, else the index of the variant. */
  key: "base" | number;
  label: string;
  score: number;
  passRate: number;
  reportId?: string;
  /** Score minus the baseline's (none for the baseline itself). */
  delta?: number;
  best: boolean;
  applied: boolean;
}

/** The baseline and the variants as table rows, in that order. */
export function optRows(o: PromptOptimization): OptRow[] {
  return [
    { key: "base", label: t("evals.prompt_attuale"), score: o.baseline_score, passRate: o.baseline_pass_rate, reportId: o.baseline_report_id, best: false, applied: false },
    ...o.variants.map((v, i): OptRow => ({
      key: i, label: t("evals.variante", { number: i + 1 }), score: v.score, passRate: v.pass_rate, reportId: v.report_id, delta: v.score - o.baseline_score,
      best: o.best === i, applied: o.applied === i,
    })),
  ];
}

/** The variant the detail shows first: the applied one, else the best, else the first. */
export const defaultVariant = (o: Pick<PromptOptimization, "applied" | "best" | "variants">): number | null =>
  o.variants.length === 0 ? null : (o.applied ?? o.best ?? 0);

/** What the result says, in a sentence; nothing is proposed when no variant beats the current prompt. */
export function verdict(o: PromptOptimization): string {
  switch (o.status) {
    case "running":
      return t("evals.verdict_running", { count: o.variants.length });
    case "failed":
      return o.error ?? t("evals.verdict_failed");
    case "applied":
      return t("evals.verdict_applied", { number: (o.applied ?? 0) + 1 });
    default:
      return o.best === undefined
        ? t("evals.verdict_none")
        : t("evals.verdict_best", { number: o.best + 1 });
  }
}

/** Can the proposal be applied? Only the best variant of a finished optimization, and only once. */
export const canApply = (o: PromptOptimization, variant: number): boolean => o.status === "done" && o.best === variant;

export type DiffLineKind = "add" | "del" | "hunk" | "head" | "ctx";

/** What a line of a unified diff is, to color it (the first two lines are the file headers). */
export function diffLineKind(line: string, index: number): DiffLineKind {
  if (index < 2) return "head";
  if (line.startsWith("@@")) return "hunk";
  if (line.startsWith("+")) return "add";
  if (line.startsWith("-")) return "del";
  return "ctx";
}
