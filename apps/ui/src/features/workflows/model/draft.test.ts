import { describe, expect, it } from "vitest";
import { workflows } from "../../../shared/api/mock/fixtures";
import type { WorkflowDef } from "../../../shared/api/types";
import { blankWorkflow, copyOf, defFromPlain, hasErrors, keyDraft, parseArgs, parseWorkflowJson, plainFromDef, previewJson, unkeyDraft, validateDraft, type PlainDraft } from "./draft";

const ctx = { agents: ["operator", "planner"], profiles: ["fast-local"], tools: ["list_dir", "shell"] };
const base = (over: Partial<PlainDraft> = {}): PlainDraft => ({
  ...blankWorkflow("wf-x", { agent: "operator" }), name: "Prova", steps: [{ id: "a", kind: "agent", agent: "operator", prompt: "fai" }], ...over,
});
const find = (issues: ReturnType<typeof validateDraft>, field: string) => issues.filter((i) => i.field === field);

describe("conversion", () => {
  it("round-trips every demo workflow without losing anything", () => {
    for (const w of workflows) {
      const back = defFromPlain(plainFromDef(w), w.trigger.kind === "webhook" ? w.trigger.secret : "");
      expect(back, w.id).toEqual(w);
    }
  });

  it("keeps the retry, timeout and error options of the steps", () => {
    const def: WorkflowDef = {
      id: "o", name: "O", enabled: false, trigger: { kind: "manual" }, inputs: {},
      steps: [{ id: "a", kind: "llm", profile: "p", prompt: "x", retry: 2, timeout_s: 30, on_error: "continue" }, { id: "b", kind: "tool", tool: "list_dir", args: {} }],
    };
    const plain = plainFromDef(def);
    expect(plain.steps[0]).toMatchObject({ opts: { retry: 2, timeout_s: 30, on_error: "continue" } });
    expect(plain.steps[1]).not.toHaveProperty("opts");
    expect(defFromPlain(plain)).toEqual(def);
    expect(find(validateDraft({ ...base(), steps: [{ id: "a", kind: "llm", profile: "fast-local", prompt: "x", opts: { retry: 9 } }] }, ctx), "retry")).toHaveLength(1);
  });

  it("keeps the time limit and default answer of an approval", () => {
    const def: WorkflowDef = {
      id: "a", name: "A", enabled: false, trigger: { kind: "manual" }, inputs: {},
      steps: [{ id: "ok", kind: "approval", message: "Procedo?", timeout_s: 600, default: "allow" }, { id: "no", kind: "approval", message: "E ora?", timeout_s: 30 }, { id: "plain", kind: "approval", message: "?" }],
    };
    const plain = plainFromDef(def);
    expect(plain.steps[0]).toMatchObject({ timeout_s: 600, default: "allow" });
    expect(plain.steps[2]).not.toHaveProperty("timeout_s");
    expect(defFromPlain(plain)).toEqual(def);
    const bad = validateDraft({ ...base(), steps: [{ id: "ok", kind: "approval", message: "?", timeout_s: 0 }] }, ctx);
    expect(find(bad, "timeout_s")).toHaveLength(1);
  });

  it("round-trips foreach and parallel steps with their inline steps", () => {
    const def: WorkflowDef = {
      id: "f", name: "F", enabled: false, trigger: { kind: "manual" }, inputs: {},
      steps: [
        { id: "each", kind: "foreach", items: "{{inputs.list}}", parallel: 4, retry: 1, do: { id: "one", kind: "llm", profile: "p", prompt: "{{item.name}} {{index}}", on_error: "continue" } },
        { id: "both", kind: "parallel", steps: [{ id: "a", kind: "tool", tool: "list_dir", args: {} }, { id: "b", kind: "agent", agent: "operator", prompt: "x" }] },
        { id: "seq", kind: "foreach", items: "uno\ndue", do: { id: "two", kind: "tool", tool: "list_dir", args: {} } },
      ],
    };
    const plain = plainFromDef(def);
    expect(plain.steps[0]).toMatchObject({ kind: "foreach", parallel: 4, opts: { retry: 1 } });
    expect(plain.steps[2]).not.toHaveProperty("parallel");
    expect(defFromPlain(plain)).toEqual(def);
    expect(parseWorkflowJson(previewJson(plain), "f")).toMatchObject({ ok: true });
  });

  it("checks the inline steps, the ids they use and the loop placeholders", () => {
    const loop = (over: object) => ({ id: "each", kind: "foreach" as const, items: "a", body: '{"id": "one", "kind": "llm", "profile": "fast-local", "prompt": "{{item}} {{item.x}} {{index}}"}', ...over });
    const run = (steps: PlainDraft["steps"]) => validateDraft({ ...base(), steps }, ctx);
    expect(run([loop({})]).filter((i) => i.level === "error")).toEqual([]);
    expect(run([loop({})]).filter((i) => i.field === "placeholder")).toEqual([]);
    expect(find(run([loop({ body: "{" })]), "body")).toHaveLength(1);
    expect(find(run([loop({ body: '{"id": "z", "kind": "approval", "message": "?"}' })]), "body")).toHaveLength(1);
    expect(find(run([loop({ items: " " })]), "items")).toHaveLength(1);
    expect(find(run([loop({ parallel: 9 })]), "parallel")).toHaveLength(1);
    expect(run([loop({}), { id: "one", kind: "agent", agent: "operator", prompt: "x" }]).some((i) => i.message.includes("duplicato"))).toBe(true);
    expect(find(run([{ id: "p", kind: "parallel", steps: "[]" }]), "steps")).toHaveLength(1);
    // {{item}} outside a loop is not known.
    expect(find(run([{ id: "a", kind: "agent", agent: "operator", prompt: "{{item}}" }]), "placeholder")).toHaveLength(1);
  });

  it("round-trips sub-workflow steps and leaves out empty inputs", () => {
    const def: WorkflowDef = {
      id: "p", name: "P", enabled: false, trigger: { kind: "manual" }, inputs: {},
      steps: [{ id: "sub", kind: "workflow", workflow: "figlio", inputs: { dir: "{{inputs.folder}}", n: 2 }, timeout_s: 60 }, { id: "bare", kind: "workflow", workflow: "altro" }],
    };
    const plain = plainFromDef(def);
    expect(plain.steps[1]).toMatchObject({ kind: "workflow", inputs: "{}" });
    expect(defFromPlain(plain)).toEqual(def);
    expect(parseWorkflowJson(previewJson(plain), "p")).toMatchObject({ ok: true });
  });

  it("checks the workflow a step starts", () => {
    const run = (workflow: string, inputs = "{}") => validateDraft({ ...base(), steps: [{ id: "s", kind: "workflow", workflow, inputs }] }, { ...ctx, workflows: ["figlio", "wf-x"], selfId: "wf-x" });
    expect(run("figlio")).toEqual([]);
    expect(find(run(""), "workflow")).toHaveLength(1);
    expect(find(run("wf-x"), "workflow")[0].message).toContain("se stesso");
    expect(find(run("sparito"), "workflow")[0].level).toBe("warn");
    expect(find(run("figlio", "[1]"), "inputs")).toHaveLength(1);
  });

  it("accepts the json and error placeholders of earlier steps and warns about unknown endings", () => {
    const steps: PlainDraft["steps"] = [
      { id: "a", kind: "agent", agent: "operator", prompt: "x" },
      { id: "b", kind: "agent", agent: "operator", prompt: "{{steps.a.json.items.0.name}} {{steps.a.error}} {{steps.a.output}}" },
      { id: "c", kind: "agent", agent: "operator", prompt: "{{steps.a.result}}" },
    ];
    const warns = find(validateDraft({ ...base(), steps }, ctx), "placeholder");
    expect(warns).toHaveLength(1);
    expect(warns[0]).toMatchObject({ step: 2, level: "warn" });
  });

  it("keeps non-string inputs as JSON and string ones as text", () => {
    const def: WorkflowDef = { id: "i", name: "I", enabled: false, trigger: { kind: "manual" }, inputs: { folder: "~/x", n: 3, flags: { a: true } }, steps: [] };
    const plain = plainFromDef(def);
    expect(plain.inputs).toEqual([{ key: "folder", value: "~/x", json: false }, { key: "n", value: "3", json: true }, { key: "flags", value: '{"a":true}', json: true }]);
    expect(defFromPlain(plain).inputs).toEqual(def.inputs);
  });

  it("leaves out an empty description and blank input keys, and trims the name", () => {
    const def = defFromPlain(base({ name: "  Prova ", description: "  ", inputs: [{ key: " ", value: "x", json: false }] }));
    expect(def).not.toHaveProperty("description");
    expect(def.name).toBe("Prova");
    expect(def.inputs).toEqual({});
  });

  it("writes an empty else as no else, as the backend does", () => {
    const d = base({ steps: [{ id: "c", kind: "condition", if: "x", then: ["a"], else: [] }, { id: "a", kind: "approval", message: "?" }] });
    expect(defFromPlain(d).steps[0]).toEqual({ id: "c", kind: "condition", if: "x", then: ["a"] });
  });

  it("turns the interval back into seconds", () => {
    const d = base({ trigger: { kind: "interval", amount: 6, unit: "h" } });
    expect(defFromPlain(d).trigger).toEqual({ kind: "interval", seconds: 21600 });
    expect(plainFromDef(defFromPlain(d)).trigger).toEqual({ kind: "interval", amount: 6, unit: "h" });
  });

  it("parses tool arguments only when they are an object", () => {
    expect(parseArgs('{"a": 1}')).toEqual({ a: 1 });
    expect(parseArgs("  ")).toEqual({});
    for (const bad of ["[1]", "3", "null", "{", '"x"']) expect(parseArgs(bad), bad).toBeNull();
  });

  it("copies a workflow as disabled, with a new id and name", () => {
    const src = plainFromDef(workflows[0]);
    const c = copyOf(src, "wf-2", [src.name, `Copia di ${src.name}`]);
    expect(c).toMatchObject({ id: "wf-2", name: `Copia di ${src.name} (2)`, enabled: false, trigger: { kind: "webhook" } });
  });

  it("carries the saved webhook secret to the wire and never into the form", () => {
    const hook = workflows.find((w) => w.trigger.kind === "webhook")!;
    const plain = plainFromDef(hook);
    expect(JSON.stringify(plain)).not.toContain("3f9a61c0");
    expect(defFromPlain(plain, "abc").trigger).toEqual({ kind: "webhook", secret: "abc" });
    expect(defFromPlain(plain).trigger).toEqual({ kind: "webhook" });
  });
});

describe("validateDraft", () => {
  it("accepts every demo workflow", () => {
    const c = { agents: ["operator", "coder", "planner", "browser"], profiles: ["fast-local"], tools: ["list_dir", "shell"] };
    for (const w of workflows) expect(hasErrors(validateDraft(plainFromDef(w), c)), w.id).toBe(false);
  });

  it("requires a name", () => {
    expect(find(validateDraft(base({ name: " " }), ctx), "name")).toHaveLength(1);
  });

  it("checks the trigger", () => {
    expect(hasErrors(validateDraft(base({ trigger: { kind: "cron", expr: "0 8 * *" } }), ctx))).toBe(true);
    expect(hasErrors(validateDraft(base({ trigger: { kind: "cron", expr: "0 8 * * 1-5" } }), ctx))).toBe(false);
    expect(hasErrors(validateDraft(base({ trigger: { kind: "interval", amount: 0, unit: "min" } }), ctx))).toBe(true);
    const fast = validateDraft(base({ trigger: { kind: "interval", amount: 10, unit: "s" } }), ctx);
    expect(hasErrors(fast)).toBe(false);
    expect(find(fast, "amount")[0].level).toBe("warn");
    expect(hasErrors(validateDraft(base({ trigger: { kind: "file", path: " " } }), ctx))).toBe(true);
  });

  it("checks the ids of the steps", () => {
    const steps: PlainDraft["steps"] = [
      { id: "a", kind: "approval", message: "?" }, { id: "a", kind: "approval", message: "?" }, { id: "b c", kind: "approval", message: "?" }, { id: "", kind: "approval", message: "?" },
    ];
    const issues = find(validateDraft(base({ steps }), ctx), "id");
    expect(issues.map((i) => [i.step, i.level])).toEqual([[1, "error"], [2, "error"], [3, "error"]]);
  });

  it("checks each kind of step", () => {
    const issues = validateDraft(base({
      steps: [
        { id: "t", kind: "tool", tool: "", args: "[1]" },
        { id: "g", kind: "agent", agent: "", prompt: "" },
        { id: "l", kind: "llm", profile: "", prompt: " " },
        { id: "p", kind: "approval", message: "" },
        { id: "c", kind: "condition", if: "", then: [], else: [] },
      ],
    }), ctx);
    const errors = issues.filter((i) => i.level === "error").map((i) => `${i.step}:${i.field}`).sort();
    expect(errors).toEqual(["0:args", "0:tool", "1:agent", "1:prompt", "2:profile", "2:prompt", "3:message", "4:if"]);
  });

  it("only warns about things that do not exist yet", () => {
    const issues = validateDraft(base({ steps: [{ id: "t", kind: "tool", tool: "ghost", args: "{}" }, { id: "g", kind: "agent", agent: "ghost", prompt: "x" }, { id: "l", kind: "llm", profile: "ghost", prompt: "x" }] }), ctx);
    expect(hasErrors(issues)).toBe(false);
    expect(issues.map((i) => i.field).sort()).toEqual(["agent", "profile", "tool"]);
  });

  it("flags condition targets that do not exist (error) or come first (warning)", () => {
    const steps: PlainDraft["steps"] = [
      { id: "a", kind: "approval", message: "?" },
      { id: "c", kind: "condition", if: "x", then: ["a", "zzz"], else: [] },
    ];
    const issues = find(validateDraft(base({ steps }), ctx), "then");
    expect(issues.map((i) => i.level)).toEqual(["warn", "error"]);
    expect(issues[1].message).toContain("zzz");
  });

  it("warns about placeholders the engine would leave empty", () => {
    const steps: PlainDraft["steps"] = [
      { id: "a", kind: "agent", agent: "operator", prompt: "{{steps.b.output}} {{steps.nope.output}} {{inputs.folder}} {{inputs.payload}} {{now}} {{what}}" },
      { id: "b", kind: "approval", message: "ok" },
    ];
    const msgs = find(validateDraft(base({ steps }), ctx), "placeholder").map((i) => i.message);
    expect(msgs).toHaveLength(4);
    expect(msgs.some((m) => m.includes("steps.b.output") && m.includes("non è ancora stato eseguito"))).toBe(true);
    expect(msgs.some((m) => m.includes("steps.nope.output") && m.includes("nessun passo"))).toBe(true);
    expect(msgs.some((m) => m.includes("folder") && m.includes("non è definito"))).toBe(true);
    expect(msgs.some((m) => m.includes("what"))).toBe(true);
  });

  it("does not worry about undeclared inputs of a webhook workflow", () => {
    const d = base({ trigger: { kind: "webhook" }, steps: [{ id: "a", kind: "agent", agent: "operator", prompt: "{{inputs.order_id}}" }] });
    expect(find(validateDraft(d, ctx), "placeholder")).toEqual([]);
  });

  it("checks the inputs", () => {
    const issues = validateDraft(base({ inputs: [{ key: "a", value: "1", json: false }, { key: "a", value: "{", json: true }, { key: "b c", value: "", json: false }] }), ctx);
    expect(issues.filter((i) => i.scope === "inputs").map((i) => i.step).sort()).toEqual([1, 1, 2]);
    expect(issues.filter((i) => i.scope === "inputs" && i.level === "error")).toHaveLength(3);
  });

  it("warns about a workflow with no steps", () => {
    const issues = validateDraft(base({ steps: [] }), ctx);
    expect(hasErrors(issues)).toBe(false);
    expect(issues.some((i) => i.scope === "steps")).toBe(true);
  });
});

describe("stable row ids", () => {
  it("gives every step and input a uid and takes it away again", () => {
    const plain = plainFromDef(workflows[1]);
    const k = keyDraft(plain);
    expect(new Set(k.steps.map((s) => s.uid)).size).toBe(k.steps.length);
    expect(unkeyDraft(k)).toEqual(plain);
  });
});

describe("JSON view", () => {
  it("masks the webhook secret in the preview", () => {
    const hook = workflows.find((w) => w.trigger.kind === "webhook")!;
    const text = previewJson(plainFromDef(hook));
    expect(text).not.toContain("3f9a61c0");
    expect(text).toContain("••••");
    expect(JSON.parse(previewJson(plainFromDef(workflows[1]))).trigger).toEqual(workflows[1].trigger);
  });

  const ok = (w: object, id = "w") => parseWorkflowJson(JSON.stringify(w), id);
  const valid = { id: "w", name: "W", trigger: { kind: "manual" }, steps: [{ id: "a", kind: "approval", message: "?" }] };

  it("reads a valid definition into a draft", () => {
    const r = ok(valid);
    expect(r.ok && r.draft).toMatchObject({ id: "w", name: "W", enabled: false, steps: [{ id: "a", kind: "approval" }] });
  });

  it("round-trips what the preview shows", () => {
    const plain = plainFromDef(workflows[2]);
    const r = parseWorkflowJson(previewJson(plain), plain.id);
    expect(r.ok && r.draft).toEqual(plain);
  });

  it("explains what is wrong", () => {
    const err = (r: ReturnType<typeof ok>) => (r.ok ? "" : r.error);
    expect(err(parseWorkflowJson("{", "w"))).toMatch(/JSON non valido/);
    expect(err(parseWorkflowJson("[]", "w"))).toMatch(/oggetto/);
    expect(err(ok({ ...valid, id: "other" }))).toMatch(/id/);
    expect(err(ok({ ...valid, name: 3 }))).toMatch(/nome/);
    expect(err(ok({ ...valid, trigger: { kind: "daily" } }))).toMatch(/trigger/);
    expect(err(ok({ ...valid, trigger: { kind: "cron" } }))).toMatch(/expr/);
    expect(err(ok({ ...valid, trigger: { kind: "interval" } }))).toMatch(/seconds/);
    expect(err(ok({ ...valid, steps: {} }))).toMatch(/elenco/);
    expect(err(ok({ ...valid, steps: [{ id: "a", kind: "magic" }] }))).toMatch(/Passo 1/);
    expect(err(ok({ ...valid, steps: [{ id: "a", kind: "tool" }] }))).toMatch(/tool/);
    expect(err(ok({ ...valid, steps: [{ id: "a", kind: "condition", if: "x", then: "b" }] }))).toMatch(/then/);
    expect(err(ok({ ...valid, inputs: [] }))).toMatch(/inputs/);
  });
});
