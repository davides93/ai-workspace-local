import { describe, expect, it } from "vitest";
import type { HookConfig } from "../../../shared/api/types";
import { blankHook, checkHooks, hookEvent, normalizeHooks } from "./hooks";

const hook = (over: Partial<HookConfig> = {}): HookConfig => ({ ...blankHook(), command: "./check.sh", ...over });
const noProblem = () => null;

describe("hook events", () => {
  it("knows which events concern a tool and which the backend really fires", () => {
    expect(hookEvent("PreToolUse")?.tools).toBe(true);
    expect(hookEvent("PostToolUse")?.tools).toBe(true);
    expect(hookEvent("Stop")?.tools).toBe(false);
    expect(hookEvent("UserPromptSubmit")?.active).toBe(true);
    expect(hookEvent("SessionStart")?.active).toBe(true);
    for (const id of ["SubagentStart", "SubagentStop"] as const) expect(hookEvent(id)).toMatchObject({ tools: true, active: true });
    expect(hookEvent("PreCompact")).toMatchObject({ tools: false, active: true });
    expect(hookEvent("Nope")).toBeUndefined();
  });
});

describe("checkHooks", () => {
  it("blocks saving on an empty command or a timeout out of range", () => {
    const issues = checkHooks([hook({ command: "  " }), hook({ timeout_s: 0 }), hook({ timeout_s: 601 }), hook({ timeout_s: 1.5 }), hook()], noProblem);
    expect(issues.filter((i) => i.level === "error").map((i) => i.index)).toEqual([0, 1, 2, 3]);
  });

  it("accepts every event the backend fires, and blocks saving on a filter the backend cannot parse as a glob", () => {
    const events = ["SessionStart", "SubagentStart", "SubagentStop", "PreCompact"] as const;
    expect(checkHooks(events.map((event) => hook({ event })), noProblem)).toEqual([]);
    const issues = checkHooks([hook({ matcher: "[x" })], (g) => (g === "[x" ? "classe [ non chiusa" : null));
    expect(issues.map((i) => [i.index, i.level])).toEqual([[0, "error"]]);
    expect(issues[0].message).toBe("Il filtro «[x» non è un glob valido (classe [ non chiusa).");
  });

  it("refuses an event the backend does not know", () => {
    const issues = checkHooks([hook({ event: "Nope" as never })], noProblem);
    expect(issues).toHaveLength(1);
    expect(issues[0]).toMatchObject({ level: "error", message: expect.stringContaining("Evento «Nope» non valido: usa UserPromptSubmit, PreToolUse") });
  });

  it("does not look at the matcher of events without a tool", () => {
    expect(checkHooks([hook({ event: "Stop", matcher: "[x" })], () => "boom")).toEqual([]);
  });
});

describe("normalizeHooks", () => {
  it("trims and drops the matcher where it does not apply", () => {
    expect(normalizeHooks([
      { event: "PreToolUse", matcher: " shell ", command: " ./a.sh ", timeout_s: 5 },
      { event: "Stop", matcher: "shell", command: "x", timeout_s: 5 },
      { event: "PostToolUse", matcher: "  ", command: "y", timeout_s: 5 },
    ])).toEqual([
      { event: "PreToolUse", matcher: "shell", command: "./a.sh", timeout_s: 5 },
      { event: "Stop", matcher: null, command: "x", timeout_s: 5 },
      { event: "PostToolUse", matcher: null, command: "y", timeout_s: 5 },
    ]);
  });
});
