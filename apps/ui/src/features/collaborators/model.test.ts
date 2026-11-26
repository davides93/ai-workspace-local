import { describe, expect, it } from "vitest";
import { MAX_TASK_INSTRUCTION_CHARS } from "../../shared/api/types";
import { problem, remoteComputer, toTask } from "./model";
import { problem, toTask } from "./model";

describe("collaborators model", () => {
  it("asks for what the collaborator should do", () => {
    expect(problem({ name: "Posta", job: "   " })).toBe("no_job");
    expect(problem({ name: "", job: "Controlla la posta ogni mattina" })).toBeUndefined();
  });
  it("counts characters, not UTF-16 units", () => {
    expect(problem({ name: "", job: "🙂".repeat(MAX_TASK_INSTRUCTION_CHARS) })).toBeUndefined();
    expect(problem({ name: "", job: "a".repeat(MAX_TASK_INSTRUCTION_CHARS + 1) })).toBe("too_long");
  });
  it("builds the task, leaving the title to the engine when no name is given", () => {
    expect(toTask({ name: "  Posta ", job: " Leggi la posta " })).toEqual({ instruction: "Leggi la posta", title: "Posta" });
    expect(toTask({ name: " ", job: "Leggi" })).toEqual({ instruction: "Leggi" });
  });
  it("names the computer only when the work runs on another one", () => {
    const device = { name: "MacBook di Dave" } as never;
    expect(remoteComputer({ mode: "daemon", device })).toBe("MacBook di Dave");
    expect(remoteComputer({ mode: "embedded", device })).toBeUndefined();
    expect(remoteComputer(undefined)).toBeUndefined();
  });
});
