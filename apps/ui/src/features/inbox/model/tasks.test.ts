import { describe, expect, it } from "vitest";
import { MAX_TASK_INSTRUCTION_CHARS } from "../../../shared/api/types";
import type { ApprovalRequest, BackgroundTask, TaskPauseReason } from "../../../shared/api/types";
import {
  announcements, appendDictated, budgetSpent, checkInstruction, commands, FAST_POLL_MS, finishedCount, finishedTasks, instructionInfo, openTasks, pauseSentence, pollInterval, refetchEvery,
  progressLine, SLOW_POLL_MS, snapshots, statusInfo, stoppingSentence, taskSummary, visualStatus,
} from "./tasks";

const task = (over: Partial<BackgroundTask> = {}): BackgroundTask => ({
  id: "t", title: "Riordinare le fatture", instruction: "Riordina", agent_id: "operator", session_id: "ses_pdf", origin: "user", status: "running",
  auto_resume: false, stopping: false, forced: false, segments: 1, run_ids: [], limits: {}, progress: { turns: 0, tokens: 0, seconds: 0 }, instructions: [], notes: [],
  created_at: "2026-11-15T08:00:00Z", updated_at: "2026-11-15T08:00:00Z", ...over,
});
const paused = (reason?: TaskPauseReason, over: Partial<BackgroundTask> = {}) => task({ status: "paused", pause_reason: reason, ...over });
const approval: ApprovalRequest = { id: "apv_1", run_id: "run_1", tool: "shell", args: {}, reason: "Sposta file", risk: "medium" };

describe("commands by state", () => {
  const only = (c: ReturnType<typeof commands>) => Object.entries(c).filter(([, on]) => on).map(([k]) => k);

  it("a task at work can be paused, told something and cancelled; nothing else", () => {
    expect(only(commands(task()))).toEqual(["pause", "cancel", "instruct"]);
  });

  it("once a pause is asked it cannot be asked again", () => {
    expect(only(commands(task({ stopping: true })))).toEqual(["cancel", "instruct"]);
  });

  it("waiting for a confirmation it can still be paused (it stops at once) and cancelled", () => {
    expect(only(commands(task({ waiting_approval: approval })))).toEqual(["pause", "cancel", "instruct"]);
  });

  it("a paused task by the person's choice or after a restart resumes plainly", () => {
    for (const reason of ["user", "restart", "error", undefined] as const) {
      expect(only(commands(paused(reason))), String(reason)).toEqual(["resume", "cancel", "instruct"]);
    }
  });

  it("a rule's pause resumes plainly or anyway", () => {
    for (const reason of ["quiet_hours", "battery", "daily_budget"] as const) {
      expect(only(commands(paused(reason))), reason).toEqual(["resume", "resumeAnyway", "cancel", "instruct"]);
    }
  });

  it("a spent budget needs more time, not a plain resume and not forcing", () => {
    expect(only(commands(paused("task_budget")))).toEqual(["addTime", "cancel", "instruct"]);
    expect(commands(paused("task_budget"), { resumeRefused: true }).resumeAnyway).toBe(false);
  });

  const spent = { limits: { max_turns: 5 }, progress: { turns: 5, tokens: 0, seconds: 0 } };

  it("the budget is read from the limits: a pause of the person or of a restart with nothing left needs more time too", () => {
    expect(budgetSpent(paused("user", spent))).toBe(true);
    expect(budgetSpent(paused("user", { limits: { max_turns: 5 }, progress: { turns: 4, tokens: 0, seconds: 0 } }))).toBe(false);
    expect(budgetSpent(paused("user", { limits: { max_seconds: 60 }, progress: { turns: 0, tokens: 0, seconds: 60 } }))).toBe(true);
    expect(budgetSpent(paused("user", { limits: { max_tokens: 100 }, progress: { turns: 0, tokens: 100, seconds: 0 } }))).toBe(true);
    expect(only(commands(paused("user", spent)))).toEqual(["addTime", "cancel", "instruct"]);
    expect(only(commands(paused("restart", spent)))).toEqual(["addTime", "cancel", "instruct"]);
  });

  it("a spent budget that a rule also holds back offers more time AND going past the rule together (the engine checks the rule first)", () => {
    // The person forced the task past the day's time, it used its own budget, and the day's time is still over.
    for (const reason of ["daily_budget", "quiet_hours", "battery"] as const) {
      const c = commands(paused(reason, spent));
      expect([c.addTime, c.addTimeAnyway, c.resume, c.resumeAnyway], reason).toEqual([true, true, false, false]);
    }
    // After a refusal with a rule's sentence the same holds for a budget pause.
    expect(commands(paused("task_budget")).addTimeAnyway).toBe(false);
    expect(commands(paused("task_budget"), { resumeRefused: true }).addTimeAnyway).toBe(true);
  });

  it("the emergency stop is never forced and no time added gets past it", () => {
    const c = commands(paused("kill_switch", spent), { resumeRefused: true });
    expect([c.addTime, c.addTimeAnyway, c.resumeAnyway]).toEqual([false, false, false]);
  });

  it("the emergency stop is never forced; the engine says what to do first", () => {
    expect(only(commands(paused("kill_switch")))).toEqual(["resume", "cancel", "instruct"]);
    expect(commands(paused("kill_switch"), { resumeRefused: true }).resumeAnyway).toBe(false);
  });

  it("a refusal for a rule offers «comunque» for a pause that did not come from a rule", () => {
    expect(commands(paused("user")).resumeAnyway).toBe(false);
    expect(commands(paused("user"), { resumeRefused: true }).resumeAnyway).toBe(true);
    expect(commands(paused("restart"), { resumeRefused: true }).resumeAnyway).toBe(true);
  });

  it("a task waiting for its turn can be resumed", () => {
    expect(only(commands(task({ status: "queued" })))).toEqual(["resume", "cancel", "instruct"]);
  });

  it("a finished task has no command and takes no instruction", () => {
    for (const status of ["succeeded", "failed", "cancelled"] as const) expect(only(commands(task({ status })))).toEqual([]);
  });
});

describe("polling", () => {
  it("asks quickly while something is at work, slowly otherwise, and never again after a 404", () => {
    expect(refetchEvery([task()], false)).toBe(FAST_POLL_MS);
    expect(refetchEvery([task({ status: "paused" })], false)).toBe(SLOW_POLL_MS);
    expect(refetchEvery(undefined, false)).toBe(SLOW_POLL_MS);
    expect(refetchEvery([task()], true)).toBe(false);
    expect(refetchEvery(undefined, true)).toBe(false);
  });
});

describe("lists, instructions and sentences", () => {
  it("open tasks keep their places while the engine saves a running one at every step (ordered by creation, not by change)", () => {
    const a = task({ id: "a", created_at: "2026-11-15T08:00:00Z", updated_at: "2026-11-15T09:00:00Z" });
    const b = task({ id: "b", created_at: "2026-11-15T08:30:00Z", updated_at: "2026-11-15T08:30:00Z" });
    expect(openTasks([a, b]).map((t) => t.id)).toEqual(["b", "a"]);
    expect(openTasks([{ ...a, updated_at: "2026-11-15T09:05:00Z" }, b]).map((t) => t.id)).toEqual(["b", "a"]);
  });

  it("an instruction is counted as the engine counts it, without the white space at the ends", () => {
    expect(checkInstruction("x".repeat(MAX_TASK_INSTRUCTION_CHARS) + "\n").ok).toBe(true);
    expect(checkInstruction("  " + "x".repeat(MAX_TASK_INSTRUCTION_CHARS + 1)).over).toBe(true);
  });

  it("an instruction that a finished task never read is «non letta», not «in attesa» for ever", () => {
    const pending = { id: "i", text: "Poi", source: "user" as const, created_at: "2026-11-15T08:00:00Z", status: "pending" as const };
    expect(instructionInfo(pending, task({ status: "running" })).label).toMatch(/attesa|waiting|pending/i);
    expect(instructionInfo(pending, task({ status: "cancelled" })).label).toMatch(/non letta|not read/i);
  });

  it("a task finishing its step says why when it was not the person who asked", () => {
    expect(stoppingSentence(task({ stopping: true, pause_reason: "user", pause_text: "Messo in pausa da te." }))).toMatch(/pausa|pause/i);
    expect(stoppingSentence(task({ stopping: true, pause_reason: "quiet_hours", pause_text: "Orario di silenzio (22:00-07:00): riprende da solo alle 07:00." })))
      .toBe("Orario di silenzio (22:00-07:00): riprende da solo alle 07:00.");
  });
});

describe("how a task looks", () => {
  it("tells waiting for a confirmation and finishing a step apart from working", () => {
    expect(visualStatus(task())).toBe("running");
    expect(visualStatus(task({ stopping: true }))).toBe("stopping");
    expect(visualStatus(task({ waiting_approval: approval }))).toBe("waiting");
    expect(visualStatus(task({ status: "paused", stopping: false }))).toBe("paused");
  });

  it("uses the tones of the other pills", () => {
    expect(statusInfo(task())).toEqual({ label: "In corso", tone: "run" });
    expect(statusInfo(paused("user"))).toEqual({ label: "In pausa", tone: "warn" });
    expect(statusInfo(task({ waiting_approval: approval }))).toEqual({ label: "Aspetta una conferma", tone: "warn" });
    expect(statusInfo(task({ status: "succeeded" })).tone).toBe("ok");
    expect(statusInfo(task({ status: "failed" })).tone).toBe("crit");
    expect(statusInfo(task({ status: "cancelled" })).tone).toBe("neutral");
    expect(statusInfo(task({ status: "queued" })).label).toBe("In coda");
  });

  it("says why it is paused: the engine's sentence first, else one by reason", () => {
    expect(pauseSentence(paused("quiet_hours", { pause_text: "  Silenzio fino alle 07:00.  " }))).toBe("Silenzio fino alle 07:00.");
    const reasons: TaskPauseReason[] = ["user", "quiet_hours", "battery", "daily_budget", "task_budget", "kill_switch", "restart", "error"];
    for (const r of reasons) expect(pauseSentence(paused(r))?.length, r).toBeGreaterThan(10);
    expect(pauseSentence(paused("restart"))).toMatch(/riavviata/);
    expect(pauseSentence(paused(undefined))).toBe("In pausa.");
    expect(pauseSentence(task())).toBeUndefined();
  });

  it("writes the progress on one line", () => {
    expect(progressLine({ turns: 3, tokens: 5200, seconds: 240, last_tool: "list_dir" })).toBe("3 passi · 4 min · ultimo strumento list_dir");
    expect(progressLine({ turns: 1, tokens: 10, seconds: 35 })).toBe("1 passo · 35 s");
    expect(progressLine({ turns: 0, tokens: 0, seconds: 0 })).toBe("Nessun passo ancora");
  });
});

describe("lists", () => {
  const list = [
    task({ id: "paused-new", status: "paused", pause_reason: "user", updated_at: "2026-11-15T09:30:00Z" }),
    task({ id: "paused-old", status: "paused", pause_reason: "restart", updated_at: "2026-11-15T07:00:00Z" }),
    task({ id: "queued", status: "queued", updated_at: "2026-11-15T09:00:00Z" }),
    task({ id: "run-old", updated_at: "2026-11-15T08:00:00Z" }),
    task({ id: "run-new", updated_at: "2026-11-15T09:00:00Z" }),
    task({ id: "asking", updated_at: "2026-11-15T06:00:00Z", waiting_approval: approval }),
    task({ id: "done", status: "succeeded", updated_at: "2026-11-15T05:00:00Z", finished_at: "2026-11-15T05:00:00Z" }),
  ];

  it("puts what needs the person first, then what is at work, queued, paused; the latest change first within each", () => {
    expect(openTasks(list).map((t) => t.id)).toEqual(["asking", "run-new", "run-old", "queued", "paused-new", "paused-old"]);
  });

  it("lists the finished ones, the latest first, at most ten", () => {
    const many = Array.from({ length: 14 }, (_, i) => task({ id: `f${i}`, status: i % 3 === 0 ? "failed" : "succeeded", updated_at: "2026-11-14T00:00:00Z", finished_at: `2026-11-14T${String(i).padStart(2, "0")}:00:00Z` }));
    const shown = finishedTasks([...many, ...list]);
    expect(shown).toHaveLength(10);
    expect(shown[0].id).toBe("done");
    expect(shown[1].id).toBe("f13");
    expect(finishedCount([...many, ...list])).toBe(15);
    expect(finishedTasks(list).map((t) => t.id)).toEqual(["done"]);
  });

  it("a finished task without finished_at is placed by its last change", () => {
    const a = task({ id: "a", status: "failed", updated_at: "2026-11-15T10:00:00Z" });
    const b = task({ id: "b", status: "cancelled", updated_at: "2026-11-15T09:00:00Z", finished_at: "2026-11-15T09:30:00Z" });
    expect(finishedTasks([b, a]).map((t) => t.id)).toEqual(["a", "b"]);
  });

  it("sums up the open ones for the overview, and says nothing when there are none", () => {
    expect(taskSummary(list)).toBe("3 lavori in corso · 3 lavori in pausa");
    expect(taskSummary([task()])).toBe("1 lavoro in corso");
    expect(taskSummary([paused("user"), task({ status: "succeeded" })])).toBe("1 lavoro in pausa");
    expect(taskSummary([task({ status: "failed" })])).toBeUndefined();
    expect(taskSummary([])).toBeUndefined();
    expect(taskSummary(undefined)).toBeUndefined();
  });

  it("refreshes fast while something is at work or stopping, slowly otherwise", () => {
    expect(pollInterval([task()])).toBe(FAST_POLL_MS);
    expect(pollInterval([paused("user", { stopping: true })])).toBe(FAST_POLL_MS);
    expect(pollInterval([paused("user"), task({ status: "succeeded" })])).toBe(SLOW_POLL_MS);
    expect(pollInterval([])).toBe(SLOW_POLL_MS);
    expect(pollInterval(undefined)).toBe(SLOW_POLL_MS);
  });
});

describe("instructions", () => {
  it("names what became of each one", () => {
    const base = { id: "i", text: "x", source: "user" as const, created_at: "2026-11-15T08:00:00Z" };
    expect(instructionInfo({ ...base, status: "pending" })).toEqual({ label: "In attesa", tone: "neutral" });
    expect(instructionInfo({ ...base, status: "delivered" })).toEqual({ label: "Letta", tone: "ok" });
    expect(instructionInfo({ ...base, status: "blocked" })).toEqual({ label: "Bloccata", tone: "crit" });
  });

  it("counts characters like the engine and refuses an empty or too long text", () => {
    expect(checkInstruction("")).toMatchObject({ count: 0, ok: false, over: false });
    expect(checkInstruction("   \n ")).toMatchObject({ ok: false });
    expect(checkInstruction("Guarda anche Documenti")).toMatchObject({ count: 22, ok: true, max: MAX_TASK_INSTRUCTION_CHARS });
    expect(checkInstruction("🙂".repeat(MAX_TASK_INSTRUCTION_CHARS))).toMatchObject({ count: MAX_TASK_INSTRUCTION_CHARS, ok: true, over: false });
    expect(checkInstruction("a".repeat(MAX_TASK_INSTRUCTION_CHARS + 1))).toMatchObject({ over: true, ok: false });
  });

  it("adds the dictated words after what is already there", () => {
    expect(appendDictated("", "  Guarda anche Documenti. ")).toBe("Guarda anche Documenti.");
    expect(appendDictated("Prima questo.  ", "poi quest'altro")).toBe("Prima questo. poi quest'altro");
    expect(appendDictated("Testo", "   ")).toBe("Testo");
  });
});

describe("announcements for screen readers", () => {
  it("says nothing on the first load", () => {
    expect(announcements(undefined, [task()])).toEqual([]);
  });

  it("says nothing when nothing changed", () => {
    const list = [task(), paused("user", { id: "p" })];
    expect(announcements(snapshots(list), list)).toEqual([]);
    // Progress alone is not worth a sentence.
    expect(announcements(snapshots(list), [task({ progress: { turns: 9, tokens: 1, seconds: 1 } }), list[1]])).toEqual([]);
  });

  it("announces each kind of change once, with the title", () => {
    const before = snapshots([task({ id: "a" }), task({ id: "b" }), task({ id: "c" }), paused("user", { id: "d" }), task({ id: "e" })]);
    const after = [
      task({ id: "a", stopping: true }),
      task({ id: "b", waiting_approval: approval }),
      task({ id: "c", status: "succeeded" }),
      task({ id: "d" }),
      task({ id: "e", status: "failed" }),
      task({ id: "f", title: "Nuovo" }),
    ];
    expect(announcements(before, after)).toEqual([
      "«Riordinare le fatture» si ferma dopo questo passo.",
      "«Riordinare le fatture» aspetta la tua conferma per shell.",
      "«Riordinare le fatture» è finito.",
      "«Riordinare le fatture» è in corso.",
      "«Riordinare le fatture» non è riuscito.",
      "Nuovo lavoro: «Nuovo».",
    ]);
  });

  it("announces a pause and a new confirmation of the same task", () => {
    const before = snapshots([task({ id: "a", waiting_approval: approval })]);
    expect(announcements(before, [task({ id: "a", status: "paused", pause_reason: "user" })])).toEqual(["«Riordinare le fatture» è in pausa."]);
    expect(announcements(before, [task({ id: "a", waiting_approval: { ...approval, id: "apv_2" } })])).toEqual(["«Riordinare le fatture» aspetta la tua conferma per shell."]);
  });
});
