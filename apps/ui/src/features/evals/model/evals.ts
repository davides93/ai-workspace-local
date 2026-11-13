import type { EvalComparison, EvalReport, EvalSuite, EvalTaskResult } from "../../../shared/api/types";
import { t } from "../../../shared/i18n";

/** The report is still being filled in: the page polls it. */
export const isRunning = (r: Pick<EvalReport, "status">): boolean => r.status === "running";

/** Poll interval of the reports, used only while one of them is still going. */
export const EVAL_POLL_MS = 2_000;

/** Longest repeat the engine accepts. */
export const MAX_REPEAT = 50;

/** Pass rate or score as a percentage of the unit: 0.756 → "76%". */
export const pct = (ratio: number): string => `${Math.round(ratio * 100)}%`;

/** A blank suite to start from in the editor. */
export function blankSuite(id = "nuova-suite"): EvalSuite {
  return {
    id,
    name: "Nuova suite",
    tasks: [{ id: "primo-task", prompt: "Crea note.md con scritto ciao", graders: [{ kind: "file_exists", path: "note.md" }] }],
  };
}

/** The suite as the editor shows it. */
export const suiteText = (s: EvalSuite): string => JSON.stringify(s, null, 2);

export type ParsedSuite = { suite: EvalSuite; error?: undefined } | { suite?: undefined; error: string };

/**
 * Reads the editor text: only the shape is checked here (an object with an id), the rest — tasks, graders,
 * agents, profiles — is the server's job, and its reason is shown as it comes.
 */
export function parseSuiteText(text: string): ParsedSuite {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch (e) {
    return { error: t("evals.json_invalid", { error: e instanceof Error ? e.message : String(e) }) };
  }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return { error: t("evals.suite_not_object") };
  const id = (v as { id?: unknown }).id;
  if (typeof id !== "string" || !id.trim()) return { error: t("evals.suite_missing_id") };
  return { suite: v as EvalSuite };
}

/** What the attempts of one task add up to. */
export interface TaskRow {
  task_id: string;
  attempts: number;
  passed: number;
  pass_rate: number;
  score: number;
  avg_tokens: number;
  avg_duration_ms: number;
  results: EvalTaskResult[];
}

/** One row per task, in the order the report ran them; the attempts are averaged. */
export function taskRows(report: Pick<EvalReport, "tasks">): TaskRow[] {
  const byTask = new Map<string, EvalTaskResult[]>();
  for (const t of report.tasks) byTask.set(t.task_id, [...(byTask.get(t.task_id) ?? []), t]);
  return [...byTask].map(([task_id, results]) => {
    const n = results.length;
    const passed = results.filter((r) => r.passed).length;
    const mean = (f: (r: EvalTaskResult) => number) => results.reduce((s, r) => s + f(r), 0) / n;
    return { task_id, attempts: n, passed, pass_rate: passed / n, score: mean((r) => r.score), avg_tokens: mean((r) => r.tokens), avg_duration_ms: mean((r) => r.duration_ms), results };
  });
}

/** How far along a running report is: attempts done out of the total (tasks × repeat). */
export function progress(report: Pick<EvalReport, "tasks" | "repeat">, suite: Pick<EvalSuite, "tasks"> | undefined): { done: number; total: number | null } {
  return { done: report.tasks.length, total: suite ? suite.tasks.length * report.repeat : null };
}

/** Who the report was made with: the agent and profile asked for, if any. */
export function reportWho(r: Pick<EvalReport, "agent" | "profile">): string {
  return [r.agent, r.profile].filter(Boolean).join(" · ") || t("evals.suite_defaults");
}

export type DeltaTone = "better" | "worse" | "same";

export const deltaTone = (delta: number | null | undefined, epsilon = 0.005): DeltaTone | null =>
  delta === null || delta === undefined ? null : delta > epsilon ? "better" : delta < -epsilon ? "worse" : "same";

/** Signed difference of two ratios in percentage points: "+25 pt", "−10 pt", "0 pt". */
export function pointsDelta(delta: number | null | undefined): string {
  if (delta === null || delta === undefined) return "—";
  const p = Math.round(delta * 100);
  return p === 0 ? "0 pt" : `${p > 0 ? "+" : "−"}${Math.abs(p)} pt`;
}

/** Signed difference of two scores: "+0,50", "−0,12". */
export function scoreDelta(delta: number | null | undefined): string {
  if (delta === null || delta === undefined) return "—";
  const v = Math.round(delta * 100) / 100;
  return v === 0 ? "0,00" : `${v > 0 ? "+" : "−"}${Math.abs(v).toFixed(2).replace(".", ",")}`;
}

/** The comparison says something about every task both reports ran, or only some. */
export const sharedTasks = (c: EvalComparison): number => c.tasks.filter((t) => t.a && t.b).length;

/** Reports of the same suite as `report` that can be compared with it, most recent first. */
export const comparableWith = (report: EvalReport, all: EvalReport[]): EvalReport[] =>
  all.filter((r) => r.id !== report.id && r.suite_id === report.suite_id && r.tasks.length > 0);
