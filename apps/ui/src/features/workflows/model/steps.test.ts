import { describe, expect, it } from "vitest";
import {
  changeKind, cleanOptions, innerIds, parseInline, optionsOf, optionsProblem, optionsSummary, dropReferences, schemaParams, freshStepId, gatedIds, newStep, placeholders, renameReferences, stepRef, stepTail, isKnownTail, stepSummary, STEP_ID,
  type StepDraft,
} from "./steps";

const steps: StepDraft[] = [
  { id: "scan", kind: "tool", tool: "list_dir", args: '{"path": "{{inputs.folder}}"}' },
  { id: "check", kind: "condition", if: "{{steps.scan.output}} contains pdf", then: ["act"], else: ["skip"] },
  { id: "act", kind: "agent", agent: "operator", prompt: "Sposta:\n{{ steps.scan.output }} {{steps.check}}" },
  { id: "skip", kind: "approval", message: "Niente da fare ({{steps.scan}})" },
];

describe("ids", () => {
  it("proposes the first free id of a kind", () => {
    expect(freshStepId("tool", [])).toBe("tool1");
    expect(freshStepId("tool", [{ id: "tool1" }, { id: "tool3" }])).toBe("tool2");
  });

  it("accepts only characters that survive in a placeholder", () => {
    for (const ok of ["scan", "step-1", "A_b2"]) expect(STEP_ID.test(ok)).toBe(true);
    for (const bad of ["", "a b", "a.b", "è", "a/b"]) expect(STEP_ID.test(bad)).toBe(false);
  });
});

describe("new steps", () => {
  it("start empty with the defaults given", () => {
    expect(newStep("agent", [], { agent: "coder" })).toEqual({ id: "agent1", kind: "agent", agent: "coder", prompt: "" });
    expect(newStep("tool", [], { tool: "shell" })).toEqual({ id: "tool1", kind: "tool", tool: "shell", args: "{}" });
    expect(newStep("llm", [], { profile: "fast" })).toMatchObject({ kind: "llm", profile: "fast" });
    expect(newStep("condition", [])).toMatchObject({ kind: "condition", then: [], else: [] });
    expect(newStep("approval", [])).toMatchObject({ kind: "approval" });
  });

  it("keep the id when the kind changes", () => {
    const s = changeKind(steps[0], "agent", steps, { agent: "operator" });
    expect(s).toEqual({ id: "scan", kind: "agent", agent: "operator", prompt: "" });
    expect(changeKind(steps[0], "tool", steps)).toBe(steps[0]);
  });
});

describe("placeholders", () => {
  it("finds the paths the engine replaces", () => {
    expect(placeholders("{{inputs.a}} e {{ steps.x.output }} {{now}} {{ }}").map((p) => [p.root, p.path])).toEqual([["inputs", "inputs.a"], ["steps", "steps.x.output"], ["now", "now"]]);
  });

  it("reads a step reference with or without .output", () => {
    expect(stepRef("steps.scan.output")).toBe("scan");
    expect(stepRef("steps.scan")).toBe("scan");
    expect(stepRef("steps.scan.output.output")).toBe("scan");
    expect(stepRef("inputs.x")).toBeNull();
  });
});

describe("renaming and removing", () => {
  it("rewrites then/else lists and placeholders, keeping the length", () => {
    const out = renameReferences(steps, "scan", "list");
    expect(out).toHaveLength(steps.length);
    expect((out[1] as Extract<StepDraft, { kind: "condition" }>).if).toBe("{{steps.list.output}} contains pdf");
    expect((out[2] as Extract<StepDraft, { kind: "agent" }>).prompt).toBe("Sposta:\n{{steps.list.output}} {{steps.check}}");
    expect((out[3] as Extract<StepDraft, { kind: "approval" }>).message).toBe("Niente da fare ({{steps.list}})");
    const cond = renameReferences(steps, "act", "do")[1] as Extract<StepDraft, { kind: "condition" }>;
    expect(cond.then).toEqual(["do"]);
    expect(cond.else).toEqual(["skip"]);
  });

  it("does not touch other steps' placeholders or unrelated ids", () => {
    const out = renameReferences(steps, "sca", "x");
    expect(out).toEqual(steps);
  });

  it("drops a removed id from the condition lists", () => {
    const cond = dropReferences(steps, "act")[1] as Extract<StepDraft, { kind: "condition" }>;
    expect(cond.then).toEqual([]);
    expect(cond.else).toEqual(["skip"]);
  });
});

describe("conditions", () => {
  it("collects the steps they steer", () => {
    expect([...gatedIds(steps)].sort()).toEqual(["act", "skip"]);
  });
});

describe("tool parameters", () => {
  it("lists the properties of a JSON Schema and marks the required ones", () => {
    expect(schemaParams({ type: "object", properties: { path: {}, pattern: {} }, required: ["path"] })).toEqual([{ name: "path", required: true }, { name: "pattern", required: false }]);
  });

  it("copes with anything else", () => {
    for (const v of [null, undefined, 3, "x", {}, { properties: null }, { properties: 3 }]) expect(schemaParams(v)).toEqual([]);
    expect(schemaParams({ properties: { a: {} }, required: "a" })).toEqual([{ name: "a", required: false }]);
  });
});

describe("summary", () => {
  it("says what each kind does on one line", () => {
    expect(stepSummary(steps[0])).toBe('list_dir {"path": "{{inputs.folder}}"}');
    expect(stepSummary(steps[1])).toBe("se {{steps.scan.output}} contains pdf → act altrimenti skip");
    expect(stepSummary({ id: "t", kind: "tool", tool: "", args: "{}" })).toBe("tool da scegliere {}");
    expect(stepSummary({ id: "c", kind: "condition", if: "", then: [], else: [] })).toBe("se … → nessun passo");
    expect(stepSummary({ id: "a", kind: "approval", message: "Procedo?" })).toBe("Procedo?");
    expect(stepSummary({ id: "a", kind: "approval", message: "Procedo?", timeout_s: 60, default: "allow" })).toBe("Procedo? (dopo 60 s: approva)");
  });
});

describe("step options", () => {
  it("drop the defaults and keep what was set", () => {
    expect(cleanOptions({ retry: 0, retry_delay_s: 2, on_error: "fail" })).toBeUndefined();
    expect(cleanOptions({ retry: 2, retry_delay_s: 5, timeout_s: 30, on_error: "continue" })).toEqual({ retry: 2, retry_delay_s: 5, timeout_s: 30, on_error: "continue" });
    expect(optionsOf({ retry: 1 })).toEqual({ retry: 1 });
  });

  it("check the limits of the backend", () => {
    expect(optionsProblem({ retry: 5, retry_delay_s: 300, timeout_s: 86_400 })).toBeNull();
    expect(optionsProblem({ retry: 6 })?.field).toBe("retry");
    expect(optionsProblem({ retry_delay_s: 301 })?.field).toBe("retry_delay_s");
    expect(optionsProblem({ timeout_s: 0 })?.field).toBe("timeout_s");
    expect(optionsProblem({ retry: 1.5 })?.field).toBe("retry");
  });

  it("are summarised and carried over when the kind changes between steps that have them", () => {
    expect(optionsSummary({ retry: 2, timeout_s: 30, on_error: "continue" })).toBe("2 nuovi tentativi · limite 30 s · prosegue se fallisce");
    expect(optionsSummary(undefined)).toBe("");
    const s: StepDraft = { id: "a", kind: "tool", tool: "x", args: "{}", opts: { retry: 1 } };
    expect(changeKind(s, "llm", [s], { profile: "p" })).toMatchObject({ kind: "llm", opts: { retry: 1 } });
    expect(changeKind(s, "approval", [s])).not.toHaveProperty("opts");
  });
});

describe("foreach and parallel", () => {
  it("start with an inline step and list the ids they bring", () => {
    const f = newStep("foreach", [], { profile: "fast" });
    expect(f).toMatchObject({ kind: "foreach", items: "" });
    expect(innerIds(f)).toEqual(["foreach1-step"]);
    const p = newStep("parallel", [], { profile: "fast" });
    expect(innerIds(p)).toEqual(["parallel1-a"]);
    expect(innerIds(steps[0])).toEqual([]);
  });

  it("accept only tool, agent and llm as inline steps", () => {
    expect(parseInline('{"id": "a", "kind": "tool", "tool": "x"}', false)).toMatchObject({ ok: true });
    expect(parseInline('[{"id": "a", "kind": "llm", "profile": "p", "prompt": "x"}, {"id": "b", "kind": "tool", "tool": "x"}]', true)).toMatchObject({ ok: true });
    expect(parseInline("{", false)).toMatchObject({ ok: false });
    expect(parseInline("[]", true)).toMatchObject({ ok: false });
    expect(parseInline('[{"id": "a", "kind": "tool"}]', false)).toMatchObject({ ok: false });
    expect(parseInline('{"id": "a", "kind": "approval", "message": "?"}', false)).toMatchObject({ ok: false, error: expect.stringContaining("niente annidamento") });
    expect(parseInline('{"id": "a b", "kind": "tool", "tool": "x"}', false)).toMatchObject({ ok: false });
    expect(parseInline('{"kind": "tool", "tool": "x"}', false)).toMatchObject({ ok: false });
  });

  it("are summarised and keep their options when the kind changes between steps that have them", () => {
    expect(stepSummary({ id: "f", kind: "foreach", items: "{{steps.scan.output}}", body: "{}", parallel: 4 })).toBe("per ogni voce di {{steps.scan.output}} (4 alla volta)");
    expect(stepSummary(newStep("parallel", [], {}))).toBe("in parallelo: parallel1-a");
    const f: StepDraft = { ...newStep("foreach", [], {}), opts: { retry: 1 } } as StepDraft;
    expect(changeKind(f, "parallel", [f], {})).toMatchObject({ kind: "parallel", opts: { retry: 1 } });
  });

  it("follow a renamed step in the list and in the inline step", () => {
    const f: StepDraft = { id: "each", kind: "foreach", items: "{{steps.scan.output}}", body: '{"id": "i", "kind": "llm", "profile": "p", "prompt": "{{steps.scan.output}} {{item}}"}' };
    const out = renameReferences([steps[0], f], "scan", "list")[1];
    expect(out).toMatchObject({ items: "{{steps.list.output}}" });
    expect((out as { body: string }).body).toContain("{{steps.list.output}} {{item}}");
  });
});

describe("sub-workflow steps", () => {
  it("start empty or with the default workflow, and say what they start", () => {
    expect(newStep("workflow", [], { workflow: "figlio" })).toEqual({ id: "workflow1", kind: "workflow", workflow: "figlio", inputs: "{}" });
    expect(stepSummary({ id: "w", kind: "workflow", workflow: "figlio", inputs: '{\n "a": "{{inputs.x}}"\n}' })).toBe('figlio { "a": "{{inputs.x}}" }');
    expect(stepSummary({ id: "w", kind: "workflow", workflow: "", inputs: "{}" })).toBe("workflow da scegliere {}");
  });

  it("follow a renamed step in their inputs", () => {
    const w: StepDraft = { id: "sub", kind: "workflow", workflow: "figlio", inputs: '{"dir": "{{steps.scan.output}}"}' };
    expect(renameReferences([steps[0], w], "scan", "list")[1]).toMatchObject({ inputs: '{"dir": "{{steps.list.output}}"}' });
  });
});

describe("JSON and error placeholders", () => {
  it("refer to the step whatever follows its id", () => {
    expect(stepRef("steps.scan.json")).toBe("scan");
    expect(stepRef("steps.scan.json.items.0.name")).toBe("scan");
    expect(stepRef("steps.scan.error")).toBe("scan");
    expect(stepTail("steps.scan.json.items.0")).toBe("json.items.0");
    expect(stepTail("steps.scan")).toBe("");
  });

  it("tell known endings from unknown ones", () => {
    for (const ok of ["", "output", "error", "json", "json.a.b", "json.items.0.name"]) expect(isKnownTail(ok), ok).toBe(true);
    for (const bad of ["jsonx", "out", "result", "errors"]) expect(isKnownTail(bad), bad).toBe(false);
  });

  it("follow a renamed step and keep the ending", () => {
    const f: StepDraft = { id: "x", kind: "llm", profile: "p", prompt: "{{steps.scan.json.items.0}} {{steps.scan.error}} {{steps.scan.output}} {{steps.scan}} {{steps.scanner.json}}" };
    expect(renameReferences([steps[0], f], "scan", "list")[1]).toMatchObject({
      prompt: "{{steps.list.json.items.0}} {{steps.list.error}} {{steps.list.output}} {{steps.list}} {{steps.scanner.json}}",
    });
  });
});
