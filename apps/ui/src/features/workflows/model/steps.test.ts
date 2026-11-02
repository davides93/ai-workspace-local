import { describe, expect, it } from "vitest";
import {
  changeKind, dropReferences, schemaParams, freshStepId, gatedIds, newStep, placeholders, renameReferences, stepRef, stepSummary, STEP_ID,
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
  });
});
