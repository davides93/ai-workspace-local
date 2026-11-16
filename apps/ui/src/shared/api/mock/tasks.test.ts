import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MAX_TASK_INSTRUCTION_CHARS } from "../types";
import { audit } from "./fixtures";
import { MockTransport } from "./index";
import { ASK_MS, READ_MS, STEP_MS, TaskStore } from "./tasks";

const RUN = "tsk_demo_run";
const QUIET = "tsk_demo_quiet";
const BUDGET = "tsk_demo_budget";
const RESTART = "tsk_demo_restart";
const DONE = "tsk_demo_done";

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-11-15T10:00:00Z"));
});
afterEach(() => vi.useRealTimers());

const later = (ms: number) => vi.setSystemTime(Date.now() + ms);

describe("background tasks in the demo", () => {
  it("starts with a task at work, three paused for different reasons and two finished, newest first", async () => {
    const api = new MockTransport();
    const list = await api.tasks();
    expect(list.map((t) => `${t.id}:${t.status}${t.pause_reason ? `/${t.pause_reason}` : ""}`)).toEqual([
      "tsk_demo_run:running", "tsk_demo_quiet:paused/quiet_hours", "tsk_demo_budget:paused/task_budget", "tsk_demo_restart:paused/restart",
      "tsk_demo_done:succeeded", "tsk_demo_failed:failed",
    ]);
    expect((await api.tasks({ status: "paused" })).map((t) => t.id)).toEqual([QUIET, BUDGET, RESTART]);
    await expect(api.tasks({ status: "boh" as never })).rejects.toMatchObject({ status: 400 });
    await expect(api.task("nope")).rejects.toMatchObject({ status: 404 });
    // The sessions behind «Apri la chat» exist in the demo.
    const { sessions } = await import("./fixtures");
    for (const t of list) expect(sessions.some((s) => s.id === t.session_id), t.id).toBe(true);
  });

  it("has no pending confirmation when it starts (the plain runs' banner must keep saying one)", async () => {
    const api = new MockTransport();
    expect(await api.approvals()).toEqual([]);
    expect((await api.tasks()).every((t) => !t.waiting_approval)).toBe(true);
  });

  it("keeps the state of each transport apart", async () => {
    const a = new MockTransport();
    const b = new MockTransport();
    await a.cancelTask(RUN);
    expect((await a.task(RUN)).status).toBe("cancelled");
    expect((await b.task(RUN)).status).toBe("running");
  });

  it("starts empty for a new installation", () => {
    expect(new TaskStore(false).list()).toEqual([]);
  });

  describe("pause", () => {
    it("a task at work finishes its step first, then stops by the person's choice", async () => {
      const api = new MockTransport();
      const asked = await api.pauseTask(RUN);
      expect(asked).toMatchObject({ status: "running", stopping: true });
      later(STEP_MS - 100);
      expect(await api.task(RUN)).toMatchObject({ status: "running", stopping: true });
      later(200);
      const t = await api.task(RUN);
      expect(t).toMatchObject({ status: "paused", pause_reason: "user", stopping: false, auto_resume: false });
      expect(t.pause_text).toMatch(/pausa/);
      // Pausing what is paused is not an error: the engine answers 200 and leaves it as it is.
      expect(await api.pauseTask(RUN)).toMatchObject({ status: "paused", pause_reason: "user" });
    });

    it("a finished task cannot be paused, cancelled twice or told anything", async () => {
      const api = new MockTransport();
      await expect(api.pauseTask(DONE)).rejects.toMatchObject({ status: 409 });
      await expect(api.cancelTask(DONE)).rejects.toMatchObject({ status: 409 });
      await expect(api.sendTaskInstruction(DONE, "Altro")).rejects.toMatchObject({ status: 409 });
      await expect(api.resumeTask(DONE)).rejects.toMatchObject({ status: 409 });
    });
  });

  describe("instructions", () => {
    it("a paused task keeps them (waiting) and reads them when it starts again", async () => {
      const api = new MockTransport();
      await api.pauseTask(RUN);
      later(STEP_MS + 1);
      const told = await api.sendTaskInstruction(RUN, "  Guarda anche Documenti  ", "voice");
      expect(told.instructions.at(-1)).toMatchObject({ text: "Guarda anche Documenti", source: "voice", status: "pending" });
      later(10_000);
      expect((await api.task(RUN)).instructions.at(-1)?.status).toBe("pending");
      await api.resumeTask(RUN);
      expect((await api.task(RUN)).instructions.at(-1)?.status).toBe("pending");
      later(READ_MS + 1);
      const read = await api.task(RUN);
      expect(read.instructions.at(-1)).toMatchObject({ status: "delivered" });
      expect(read.instructions.at(-1)?.delivered_at).toBeTruthy();
      expect(read.notes.at(-1)?.text).toMatch(/Ha letto la tua istruzione/);
    });

    it("a task at work reads one at its next step", async () => {
      const api = new MockTransport();
      await api.sendTaskInstruction(QUIET, "Poi dimmi il più conveniente");
      await api.sendTaskInstruction(RUN, "Fai in fretta");
      expect((await api.task(RUN)).instructions.at(-1)?.status).toBe("pending");
      later(READ_MS + 1);
      expect((await api.task(RUN)).instructions.at(-1)?.status).toBe("delivered");
    });

    it("refuses what the engine refuses", async () => {
      const api = new MockTransport();
      await expect(api.sendTaskInstruction(RUN, "   ")).rejects.toMatchObject({ status: 400 });
      await expect(api.sendTaskInstruction(RUN, "a".repeat(MAX_TASK_INSTRUCTION_CHARS + 1))).rejects.toMatchObject({ status: 400 });
      await expect(api.sendTaskInstruction(RUN, "ok", "robot" as never)).rejects.toMatchObject({ status: 400 });
      // It counts characters, not bytes: 4000 emoji are fine.
      await expect(api.sendTaskInstruction(RUN, "🙂".repeat(MAX_TASK_INSTRUCTION_CHARS))).resolves.toMatchObject({ id: RUN });
      await expect(api.sendTaskInstruction("nope", "ok")).rejects.toMatchObject({ status: 404 });
    });

    it("an instruction that was never read stays «pending» when the task is cancelled, as in the engine", async () => {
      const api = new MockTransport();
      await api.sendTaskInstruction(QUIET, "Poi dimmi il più conveniente");
      const t = await api.cancelTask(QUIET);
      expect(t.instructions.at(-1)).toMatchObject({ status: "pending" });
    });

    it("at most ten instructions wait at once, as in the engine", async () => {
      const api = new MockTransport();
      for (let i = 0; i < 10; i++) await api.sendTaskInstruction(QUIET, `Istruzione ${i}`);
      await expect(api.sendTaskInstruction(QUIET, "Una di troppo")).rejects.toMatchObject({ status: 409 });
    });
  });

  describe("resume", () => {
    it("is refused in quiet hours, with the sentence that points to «Riprendi comunque»", async () => {
      const api = new MockTransport();
      const err = await api.resumeTask(QUIET).catch((e: unknown) => e);
      expect(err).toMatchObject({ status: 409 });
      expect((err as Error).message).toMatch(/orario di silenzio/);
      expect((err as Error).message).toMatch(/Riprendi comunque/);
      expect((await api.task(QUIET)).status).toBe("paused");
    });

    it("goes ahead when forced, and says so", async () => {
      const api = new MockTransport();
      const t = await api.resumeTask(QUIET, { force: true });
      expect(t).toMatchObject({ status: "running", forced: true, auto_resume: false, segments: 2 });
      expect(t.pause_reason).toBeUndefined();
      expect(t.pause_text).toBeUndefined();
      expect(t.run_ids).toHaveLength(2);
      expect(t.current_run_id).toBe(t.run_ids[1]);
    });

    it("battery and the day's time are refused the same way; a rule that ended lets a plain resume through", async () => {
      const store = new TaskStore();
      store.rules.quiet_hours = false;
      expect(store.resume(QUIET)).toMatchObject({ status: "running", forced: false });
      store.tasks.find((t) => t.id === RESTART)!.pause_reason = "battery";
      expect(() => store.resume(RESTART)).toThrow(/batteria/);
      store.tasks.find((t) => t.id === RESTART)!.pause_reason = "daily_budget";
      expect(() => store.resume(RESTART)).toThrow(/oggi/);
    });

    it("needs more budget for a task that used its own, and adds exactly what was asked", async () => {
      const api = new MockTransport();
      const err = await api.resumeTask(BUDGET).catch((e: unknown) => e);
      expect(err).toMatchObject({ status: 409 });
      expect((err as Error).message).toMatch(/budget/);
      // Forcing is not the way out of an exhausted budget.
      await expect(api.resumeTask(BUDGET, { force: true })).rejects.toMatchObject({ status: 409 });
      await expect(api.resumeTask(BUDGET, { extend: { max_seconds: 0 } })).rejects.toMatchObject({ status: 400 });
      const t = await api.resumeTask(BUDGET, { extend: { max_seconds: 600 } });
      expect(t.status).toBe("running");
      expect(t.limits.max_seconds).toBe(2400);
    });

    it("after a restart nothing starts by itself, and the person resumes without forcing anything", async () => {
      const api = new MockTransport();
      const before = await api.task(RESTART);
      expect(before).toMatchObject({ status: "paused", pause_reason: "restart", auto_resume: false });
      expect(await api.resumeTask(RESTART)).toMatchObject({ status: "running", forced: false });
    });

    it("the emergency button stops what is at work, and not even forcing brings it back", async () => {
      const api = new MockTransport();
      await api.kill();
      const t = await api.task(RUN);
      expect(t).toMatchObject({ status: "paused", pause_reason: "kill_switch" });
      await expect(api.resumeTask(RUN, { force: true })).rejects.toMatchObject({ status: 409 });
    });

    it("every pause is refused in quiet hours when the rule covers the whole night", async () => {
      const store = new TaskStore();
      store.rules.quietNow = true;
      expect(() => store.resume(RESTART)).toThrow(/Riprendi comunque/);
      expect(store.resume(RESTART, { force: true })).toMatchObject({ status: "running", forced: true });
    });

    it("a task that is not paused cannot be resumed", async () => {
      const api = new MockTransport();
      await expect(api.resumeTask(RUN)).rejects.toMatchObject({ status: 409 });
    });
  });

  describe("confirmations", () => {
    it("the resumed task asks for one, then for a second; allowing and denying both let it go on", async () => {
      const api = new MockTransport();
      await api.pauseTask(RUN);
      later(STEP_MS + 1);
      await api.resumeTask(RUN);
      expect(await api.approvals()).toEqual([]);
      later(ASK_MS + 1);
      const [first] = await api.approvals();
      expect(first).toMatchObject({ tool: "shell", risk: "medium" });
      const waiting = await api.task(RUN);
      expect(waiting.waiting_approval).toEqual(first);
      expect(first.run_id).toBe(waiting.current_run_id);

      const before = audit.length;
      await api.respondApproval(first.id, "allow");
      expect(audit.length).toBe(before + 1);
      expect(audit[0]).toMatchObject({ action: "approval", tool: "shell", decision: "approved" });
      expect(await api.approvals()).toEqual([]);
      later(READ_MS + 1);
      const [second] = await api.approvals();
      expect(second).toMatchObject({ tool: "write_file", risk: "low" });

      await api.respondApproval(second.id, "deny");
      later(10_000);
      expect(await api.approvals()).toEqual([]);
      const t = await api.task(RUN);
      expect(t.status).toBe("running");
      expect(t.waiting_approval).toBeUndefined();
      expect(t.progress.last_tool).toBe("write_file");
      expect(t.notes.at(-1)?.text).toMatch(/negato/);
    });

    it("a pause while it waits is immediate, and the confirmation comes back when it resumes", async () => {
      const api = new MockTransport();
      await api.pauseTask(RUN);
      later(STEP_MS + 1);
      await api.resumeTask(RUN);
      later(ASK_MS + 1);
      const [asked] = await api.approvals();
      const paused = await api.pauseTask(RUN);
      expect(paused).toMatchObject({ status: "paused", stopping: false, pause_reason: "user" });
      expect(paused.waiting_approval).toBeUndefined();
      expect(await api.approvals()).toEqual([]);
      await api.resumeTask(RUN);
      later(ASK_MS + 1);
      expect((await api.approvals()).map((a) => a.tool)).toEqual([asked.tool]);
    });

    it("an answer to a request nobody asked is ignored, as for the plain runs", async () => {
      const api = new MockTransport();
      await expect(api.respondApproval("apv_nope", "allow")).resolves.toBeUndefined();
    });
  });

  describe("cancel", () => {
    it("ends the task for good", async () => {
      const api = new MockTransport();
      const t = await api.cancelTask(RUN);
      expect(t).toMatchObject({ status: "cancelled", stopping: false });
      expect(t.finished_at).toBeTruthy();
      await expect(api.cancelTask(RUN)).rejects.toMatchObject({ status: 409 });
      await expect(api.resumeTask(RUN)).rejects.toMatchObject({ status: 409 });
    });
  });

  describe("create", () => {
    it("starts a task in a new session with the title taken from the first line", async () => {
      const api = new MockTransport();
      const t = await api.createTask({ instruction: "Ordina le foto di agosto\ne mettile in un album", limits: { max_seconds: 900 } });
      expect(t).toMatchObject({ status: "running", origin: "user", agent_id: "operator", title: "Ordina le foto di agosto", limits: { max_seconds: 900 } });
      expect((await api.sessions()).some((s) => s.id === t.session_id)).toBe(true);
      expect((await api.tasks())[0].id).toBe(t.id);
    });

    it("refuses an empty instruction, an unknown agent and a limit below 1", async () => {
      const api = new MockTransport();
      await expect(api.createTask({ instruction: "  " })).rejects.toMatchObject({ status: 400 });
      await expect(api.createTask({ instruction: "x", agent_id: "nessuno" })).rejects.toMatchObject({ status: 404 });
      await expect(api.createTask({ instruction: "x", limits: { max_turns: 0 } })).rejects.toMatchObject({ status: 400 });
    });
  });
});
