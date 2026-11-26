import { describe, expect, it } from "vitest";
import { MAX_TASK_INSTRUCTION_CHARS } from "../../shared/api/types";
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
});
