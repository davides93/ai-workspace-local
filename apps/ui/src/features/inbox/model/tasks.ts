import { MAX_TASK_INSTRUCTION_CHARS } from "../../../shared/api/types";
import type { BackgroundTask, TaskInstruction, TaskInstructionStatus, TaskPauseReason, TaskProgress } from "../../../shared/api/types";
import { formatDuration } from "../../../shared/lib/format";
import type { PillTone } from "../../../shared/ui";

import { t as tr } from "../../../shared/i18n";
// Background tasks: what the assistant does while the person is elsewhere. Pure functions of the task the engine sent.

/** Page refresh while something is at work, and when nothing is (the same pace as the proposals). */
export const FAST_POLL_MS = 4_000;
export const SLOW_POLL_MS = 20_000;
/** «Aggiungi tempo» adds this much (10 minutes) to a task that used its own budget. */
export const ADD_TIME_SECONDS = 600;
/**
 * What «Aggiungi tempo» sends: the engine raises only the limits the task has, so the steps and the tokens that go with ten
 * minutes are asked for too (a task that stopped at its step limit would not go on with more seconds alone).
 */
export const ADD_TIME = { max_seconds: ADD_TIME_SECONDS, max_turns: 30, max_tokens: 50_000 } as const;
/** How many finished tasks the page lists. */
export const FINISHED_SHOWN = 10;

export const isFinished = (t: BackgroundTask): boolean => t.status === "succeeded" || t.status === "failed" || t.status === "cancelled";

/** At work now, or finishing the step before a pause: the page refreshes quickly. */
export const isWorking = (t: BackgroundTask): boolean => t.status === "running" || t.stopping;

export const pollInterval = (list: BackgroundTask[] | undefined): number => (list?.some(isWorking) ? FAST_POLL_MS : SLOW_POLL_MS);

/** Pauses of the rules of the proactive assistant: they end by themselves, and the person may go past them. */
const RULE_PAUSES: readonly TaskPauseReason[] = ["quiet_hours", "battery", "daily_budget"];

export interface TaskCommands {
  pause: boolean;
  resume: boolean;
  /** Goes past quiet hours, battery or the day's time. */
  resumeAnyway: boolean;
  /** The task used its own budget: resuming needs more of it. */
  addTime: boolean;
  cancel: boolean;
  /** Something can still be said to it. */
  instruct: boolean;
}

/**
 * What the person can do with a task, by its state.
 * - «Pausa» only while it works, and not twice (once asked it is finishing the step).
 * - «Riprendi» when paused or waiting for its turn; not for a spent budget (only more time brings it back). After the
 *   emergency button it stays: the engine answers with what to do first.
 * - «Riprendi comunque» for the rules; also, after the engine refused a resume with a rule's sentence (`resumeRefused`:
 *   a task the person paused during quiet hours is refused too), for any pause that is not the emergency stop or a spent budget.
 */
export function commands(t: BackgroundTask, opts: { resumeRefused?: boolean } = {}): TaskCommands {
  const open = !isFinished(t);
  const paused = t.status === "paused" || t.status === "queued";
  const reason = t.pause_reason;
  const budget = t.status === "paused" && reason === "task_budget";
  const stoppedByButton = reason === "kill_switch";
  return {
    pause: t.status === "running" && !t.stopping,
    resume: paused && !budget,
    resumeAnyway: t.status === "paused" && !budget && !stoppedByButton && ((reason !== undefined && RULE_PAUSES.includes(reason)) || !!opts.resumeRefused),
    addTime: budget,
    cancel: open,
    instruct: open,
  };
}

export type TaskVisualStatus = "queued" | "running" | "stopping" | "waiting" | "paused" | "succeeded" | "failed" | "cancelled";

/** The state as the person reads it: waiting for a confirmation and finishing a step are shown apart from "at work". */
export function visualStatus(t: BackgroundTask): TaskVisualStatus {
  if (t.status === "running" && t.waiting_approval) return "waiting";
  if (t.status === "running" && t.stopping) return "stopping";
  return t.status;
}

const STATUS_TONE: Record<TaskVisualStatus, PillTone> = {
  queued: "neutral", running: "run", stopping: "warn", waiting: "warn", paused: "warn", succeeded: "ok", failed: "crit", cancelled: "neutral",
};

export function statusInfo(t: BackgroundTask): { label: string; tone: PillTone } {
  const s = visualStatus(t);
  return { label: tr(`inbox.task_status_${s}`), tone: STATUS_TONE[s] };
}

/** Why it is paused, in a sentence: the engine's own, else one by reason. Nothing when it is not paused. */
export function pauseSentence(t: BackgroundTask): string | undefined {
  if (t.status !== "paused") return undefined;
  const text = t.pause_text?.trim();
  if (text) return text;
  return t.pause_reason ? tr(`inbox.task_pause_${t.pause_reason}`) : tr("inbox.task_paused_generic");
}

/** «3 passi · 4 min · ultimo strumento list_dir». */
export function progressLine(p: TaskProgress): string {
  const parts = [tr("inbox.task_progress_steps", { n: p.turns })];
  if (p.seconds > 0) parts.push(formatDuration(p.seconds));
  if (p.last_tool) parts.push(tr("inbox.task_progress_tool", { tool: p.last_tool }));
  return parts.join(" · ");
}

// ---- the lists

/** What needs the person comes first: a confirmation, then what is at work, what waits for its turn, what is paused. */
const rank = (t: BackgroundTask): number => (t.waiting_approval ? 0 : t.status === "running" ? 1 : t.status === "queued" ? 2 : 3);

/** Not finished: at work, waiting, paused. The newest change first within the same kind. */
export function openTasks(list: BackgroundTask[]): BackgroundTask[] {
  return list.filter((t) => !isFinished(t)).sort((a, b) => rank(a) - rank(b) || b.updated_at.localeCompare(a.updated_at) || a.id.localeCompare(b.id));
}

/** Finished, the latest first (`limit` of them). */
export function finishedTasks(list: BackgroundTask[], limit: number = FINISHED_SHOWN): BackgroundTask[] {
  return list
    .filter(isFinished)
    .sort((a, b) => (b.finished_at ?? b.updated_at).localeCompare(a.finished_at ?? a.updated_at) || a.id.localeCompare(b.id))
    .slice(0, limit);
}

export const finishedCount = (list: BackgroundTask[]): number => list.filter(isFinished).length;

/** The line of the overview: «2 lavori in corso · 1 lavoro in pausa». Nothing when no task is open. */
export function taskSummary(list: BackgroundTask[] | undefined): string | undefined {
  const open = (list ?? []).filter((t) => !isFinished(t));
  const running = open.filter((t) => t.status === "running").length;
  const paused = open.length - running;
  const parts: string[] = [];
  if (running > 0) parts.push(tr("inbox.task_summary_running", { n: running }));
  if (paused > 0) parts.push(tr("inbox.task_summary_paused", { n: paused }));
  return parts.length ? parts.join(" · ") : undefined;
}

// ---- instructions

const INSTRUCTION_TONE: Record<TaskInstructionStatus, PillTone> = { pending: "neutral", delivered: "ok", blocked: "crit" };

export function instructionInfo(i: TaskInstruction): { label: string; tone: PillTone } {
  return { label: tr(`inbox.task_instr_${i.status}`), tone: INSTRUCTION_TONE[i.status] };
}

export interface InstructionCheck {
  /** Characters, as the engine counts them (a pair of UTF-16 units is one). */
  count: number;
  max: number;
  over: boolean;
  /** Can be sent: something is written and it is not too long. */
  ok: boolean;
}

export function checkInstruction(text: string): InstructionCheck {
  const count = [...text].length;
  const over = count > MAX_TASK_INSTRUCTION_CHARS;
  return { count, max: MAX_TASK_INSTRUCTION_CHARS, over, ok: text.trim().length > 0 && !over };
}

/** What the dictation adds to the box: the dictated words after what is already written, with one space between. */
export const appendDictated = (text: string, dictated: string): string => {
  const add = dictated.trim();
  if (!add) return text;
  return text.trim() ? `${text.replace(/\s+$/, "")} ${add}` : add;
};

// ---- announcements for screen readers

/** What an announcement is about: the state, whether it is stopping, and the confirmation it waits for. */
const snapshot = (t: BackgroundTask): string => `${t.status}|${t.stopping}|${t.waiting_approval?.id ?? ""}`;

export type TaskSnapshots = ReadonlyMap<string, string>;

export const snapshots = (list: BackgroundTask[]): TaskSnapshots => new Map(list.map((t) => [t.id, snapshot(t)]));

/**
 * The sentences to announce when `list` differs from the state before it (`prev`). The first load announces nothing
 * (`prev` undefined): the page is read as a page. One sentence per changed task.
 */
export function announcements(prev: TaskSnapshots | undefined, list: BackgroundTask[]): string[] {
  if (!prev) return [];
  const out: string[] = [];
  for (const t of list) {
    const before = prev.get(t.id);
    if (before === snapshot(t)) continue;
    const p = { title: t.title };
    if (before === undefined) out.push(tr("inbox.task_ann_new", p));
    else if (isFinished(t)) out.push(tr(`inbox.task_ann_${t.status}` as "inbox.task_ann_succeeded", p));
    else if (t.waiting_approval) out.push(tr("inbox.task_ann_waiting", { ...p, tool: t.waiting_approval.tool }));
    else if (t.stopping) out.push(tr("inbox.task_ann_stopping", p));
    else out.push(tr(`inbox.task_ann_${t.status}` as "inbox.task_ann_running", p));
  }
  return out;
}
