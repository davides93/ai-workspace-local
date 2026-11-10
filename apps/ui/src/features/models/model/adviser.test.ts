import { describe, expect, it } from "vitest";
import type { CatalogModel, ModelChoice, Recommendations, TaskAdvice } from "../../../shared/api/types";
import { autoChoices, autoProfileFromAdvice, autoSummary, autoTask, catalogForChoice, hiddenCount, isAutoProfile, runnableOnly } from "./adviser";
import { catalogEntry } from "./testdata";

const choice = (over: Partial<ModelChoice> = {}): ModelChoice => ({
  model: "qwen3-8b-q4_k_m", name: "Qwen3 8B", runtime_id: "llama-server", context: 8192, fit: "great", size_bytes: 5e9, reason: "Sa usare bene gli strumenti.", ...over,
});
const advice = (installed: ModelChoice | null, download: ModelChoice | null = null): TaskAdvice => ({ installed, download });
const rec = (agent: TaskAdvice): Recommendations => ({ tier: "m", chat: advice(null), agent, reasoning: advice(null), vision: advice(null), embeddings: advice(null) });

describe("auto profiles", () => {
  it("recognises auto:<task> ids and nothing else", () => {
    expect(autoTask("auto:agent")).toBe("agent");
    expect(autoTask("auto:vision")).toBe("vision");
    expect(autoTask("auto:embeddings")).toBeUndefined();
    expect(autoTask("auto:nope")).toBeUndefined();
    expect(autoTask("default")).toBeUndefined();
    expect(autoTask(undefined)).toBeUndefined();
    expect(isAutoProfile("auto:chat")).toBe(true);
    expect(isAutoProfile("autonomo")).toBe(false);
  });
  it("offers 'Automatico (consigliato)' first and mapped to auto:agent", () => {
    const simple = autoChoices("simple");
    expect(simple).toEqual([{ value: "auto:agent", label: "Automatico (consigliato)" }]);
    expect(autoChoices("developer").map((c) => c.value)).toEqual(["auto:agent", "auto:chat", "auto:reasoning", "auto:vision"]);
  });
  it("keeps showing the automatic profile an agent already has, even in simple mode", () => {
    expect(autoChoices("simple", "auto:reasoning").map((c) => c.value)).toEqual(["auto:agent", "auto:reasoning"]);
    expect(autoChoices("simple", "default").map((c) => c.value)).toEqual(["auto:agent"]);
  });
});

describe("autoSummary", () => {
  it("says which model it uses now", () => {
    expect(autoSummary("auto:agent", rec(advice(choice())))).toBe("Ora usa Qwen3 8B. Sa usare bene gli strumenti.");
  });
  it("points at the download when nothing installed is suited", () => {
    expect(autoSummary("auto:agent", rec(advice(null, choice({ name: "Qwen3 4B" }))))).toContain("scarica Qwen3 4B da Modelli");
    expect(autoSummary("auto:agent", rec(advice(null)))).toContain("Nessun modello installato");
  });
  it("is generic without the adviser and empty for a normal profile", () => {
    expect(autoSummary("auto:agent", null)).toContain("Sceglie da solo");
    expect(autoSummary("default", rec(advice(choice())))).toBeUndefined();
  });
});

describe("catalogForChoice", () => {
  const list: CatalogModel[] = [catalogEntry({ id: "a" }), catalogEntry({ id: "b", installed: true })];
  it("finds the downloadable entry of a proposal", () => {
    expect(catalogForChoice(choice({ catalog_id: "a" }), list)?.id).toBe("a");
  });
  it("ignores unknown, installed or id-less proposals", () => {
    expect(catalogForChoice(choice({ catalog_id: "zz" }), list)).toBeUndefined();
    expect(catalogForChoice(choice({ catalog_id: "b" }), list)).toBeUndefined();
    expect(catalogForChoice(choice(), list)).toBeUndefined();
    expect(catalogForChoice(null, list)).toBeUndefined();
  });
});

describe("runnableOnly", () => {
  const list = [{ id: 1, fit: "great" }, { id: 2, fit: "no" }, { id: 3, fit: "tight" }, { id: 4, fit: "" }];
  it("hides what cannot run here until 'Mostra tutti'", () => {
    expect(runnableOnly(list, false).map((m) => m.id)).toEqual([1, 3, 4]);
    expect(runnableOnly(list, true)).toHaveLength(4);
    expect(hiddenCount(list)).toBe(1);
  });
});

describe("autoProfileFromAdvice", () => {
  it("builds the profile the engine will resolve to", () => {
    const r = rec(advice(choice({ thinking: "off", context: 16384 })));
    expect(autoProfileFromAdvice("auto:agent", r)).toEqual({
      id: "auto:agent", name: "Automatico (consigliato)", runtime_id: "llama-server", model: "qwen3-8b-q4_k_m", params: { context: 16384, thinking: "off" },
    });
    expect(autoProfileFromAdvice("auto:agent", rec(advice(choice())))?.params).toEqual({ context: 8192 });
  });
  it("is null for other profiles, no adviser or nothing installed", () => {
    expect(autoProfileFromAdvice("default", rec(advice(choice())))).toBeNull();
    expect(autoProfileFromAdvice("auto:agent", null)).toBeNull();
    expect(autoProfileFromAdvice("auto:agent", rec(advice(null)))).toBeNull();
    expect(autoProfileFromAdvice("auto:vision", rec(advice(choice())))).toBeNull();
  });
});
