import { describe, expect, it } from "vitest";
import { agents, profiles } from "../../../shared/api/mock/fixtures";
import type { AgentProfile } from "../../../shared/api/types";
import {
  BUILTIN_AGENT_IDS, NEW_AGENT, agentsUsingProfile, blankAgent, copyName, duplicateAgent, estimateTokens, hasErrors, idForNew, isBuiltinAgent,
  cleanBudget, normalizeAgent, withKnowledge, parseSchemaText, schemaToText, slugify, uniqueId, unknownTools, validateAgent,
} from "./agents";

const ctx = { profileIds: profiles.map((p) => p.id), otherNames: [] as string[] };
const valid = (over: Partial<AgentProfile> = {}): AgentProfile => ({ ...blankAgent("balanced"), name: "Revisore", ...over });

describe("built-in agents", () => {
  it("are the six seeded by the engine", () => {
    expect(BUILTIN_AGENT_IDS).toEqual(["operator", "coder", "browser", "desktop", "planner", "teacher"]);
    expect(isBuiltinAgent("coder")).toBe(true);
    expect(isBuiltinAgent("revisore")).toBe(false);
  });

  it("match the demo fixtures", () => {
    expect(agents.map((a) => a.id).sort()).toEqual([...BUILTIN_AGENT_IDS].sort());
  });
});

describe("ids", () => {
  it("slugifies names without accents or punctuation", () => {
    expect(slugify("Revisore di codice")).toBe("revisore-di-codice");
    expect(slugify("  Più città!  ")).toBe("piu-citta");
    expect(slugify("???")).toBe("agente");
    expect(slugify("???", "profilo")).toBe("profilo");
    expect(slugify("a".repeat(80)).length).toBeLessThanOrEqual(40);
  });

  it("makes ids unique and never uses the reserved one", () => {
    expect(uniqueId("coder", ["coder"])).toBe("coder-2");
    expect(uniqueId("coder", ["coder", "coder-2"])).toBe("coder-3");
    expect(uniqueId("coder", [])).toBe("coder");
    expect(uniqueId(NEW_AGENT, [])).toBe("new-2");
    expect(idForNew("Coder", agents)).toBe("coder-2");
    expect(idForNew("Nuovo", agents)).toBe("nuovo");
  });
});

describe("new and duplicated agents", () => {
  it("starts read-only with the plan tool, asking before anything else", () => {
    const a = blankAgent("balanced");
    expect(a).toMatchObject({ mode: "ask", sandbox: "workspace-write", inference_profile: "balanced" });
    expect(a.tools).toContain("todo");
    expect(a.tools).not.toContain("shell");
  });

  it("copies an agent under a fresh name and without an id", () => {
    const coder = agents.find((a) => a.id === "coder")!;
    const copy = duplicateAgent(coder, agents);
    expect(copy).toMatchObject({ id: "", name: "Copia di Coder", tools: ["*"], mode: coder.mode });
    copy.tools.push("x");
    expect(coder.tools).toEqual(["*"]);
    expect(copyName("Coder", ["Copia di Coder"])).toBe("Copia di Coder (2)");
    expect(copyName("Coder", ["copia di coder", "Copia di Coder (2)"])).toBe("Copia di Coder (3)");
  });
});

describe("knowledge", () => {
  it("ticks and unticks a collection, without duplicates", () => {
    const a = withKnowledge(withKnowledge(valid(), "k1", true), "k2", true);
    expect(a.knowledge).toEqual(["k1", "k2"]);
    expect(withKnowledge(a, "k1", true).knowledge).toEqual(["k2", "k1"]);
    expect(withKnowledge(a, "k1", false).knowledge).toEqual(["k2"]);
  });
  it("drops an empty list and repeated ids when normalizing", () => {
    expect("knowledge" in normalizeAgent(valid({ knowledge: [] }))).toBe(false);
    expect(normalizeAgent(valid({ knowledge: ["k1", "k1"] })).knowledge).toEqual(["k1"]);
  });
});

describe("normalizeAgent", () => {
  it("trims, dedupes the tools and clamps the turns", () => {
    const n = normalizeAgent(valid({ name: "  Revisore ", description: " x ", tools: ["a", "a", "*"], max_turns: 9000 }));
    expect(n).toMatchObject({ name: "Revisore", description: "x", tools: ["*"], max_turns: 500 });
    expect(normalizeAgent(valid({ max_turns: 0 })).max_turns).toBe(1);
    // Candidate profiles are listed once, and an empty list is left out of the saved agent.
    expect(normalizeAgent(valid({ candidate_profiles: ["a", "b", "a"] })).candidate_profiles).toEqual(["a", "b"]);
    expect("candidate_profiles" in normalizeAgent(valid({ candidate_profiles: [] }))).toBe(false);
    expect(normalizeAgent(valid({ max_turns: Number.NaN })).max_turns).toBe(30);
  });
});

describe("validateAgent", () => {
  it("accepts a sound agent", () => {
    expect(validateAgent(valid(), ctx)).toEqual([]);
  });

  it("requires a name and a profile that exists", () => {
    const issues = validateAgent(valid({ name: " ", inference_profile: "ghost" }), ctx);
    expect(issues.filter((i) => i.level === "error").map((i) => i.field).sort()).toEqual(["inference_profile", "name"]);
    expect(hasErrors(issues)).toBe(true);
    expect(hasErrors(validateAgent(valid({ inference_profile: "" }), ctx))).toBe(true);
  });

  it("checks the turns", () => {
    for (const max_turns of [0, 501, Number.NaN]) expect(hasErrors(validateAgent(valid({ max_turns }), ctx))).toBe(true);
    expect(hasErrors(validateAgent(valid({ max_turns: 500 }), ctx))).toBe(false);
  });

  it("rejects a profile that does not exist even when no profile is left, once the list is loaded", () => {
    const issues = validateAgent(valid({ inference_profile: "balanced" }), { profileIds: [], otherNames: [], profilesLoaded: true });
    expect(issues.map((i) => i.field)).toContain("inference_profile");
    expect(hasErrors(issues)).toBe(true);
    // Not loaded yet: nothing to say.
    expect(validateAgent(valid({ inference_profile: "balanced" }), { profileIds: [], otherNames: [], profilesLoaded: false })).toEqual([]);
  });

  it("accepts an automatic profile, which is not a stored one", () => {
    for (const id of ["auto:agent", "auto:chat", "auto:reasoning", "auto:vision"]) {
      expect(validateAgent(valid({ inference_profile: id }), { profileIds: [], otherNames: [], profilesLoaded: true })).toEqual([]);
    }
    // Only the four tasks exist.
    expect(hasErrors(validateAgent(valid({ inference_profile: "auto:nope" }), { profileIds: [], otherNames: [], profilesLoaded: true }))).toBe(true);
  });

  it("lists the tools the catalogue does not know, as the backend does (MCP tools and * are always fine)", () => {
    const known = ["read_file", "todo"];
    expect(unknownTools(["*", "read_file", "mcp__gone__x", "writ_file", "ghost"], known)).toEqual(["writ_file", "ghost"]);
    const issues = validateAgent(valid({ tools: ["read_file", "writ_file", "ghost"] }), { ...ctx, knownTools: known });
    const bad = issues.find((i) => i.field === "tools" && i.level === "error");
    expect(bad?.message).toMatch(/writ_file, ghost/);
    expect(hasErrors(validateAgent(valid({ tools: ["*", "mcp__gone__x"] }), { ...ctx, knownTools: known }))).toBe(false);
    // Catalogue not loaded: nothing is flagged.
    expect(hasErrors(validateAgent(valid({ tools: ["ghost"] }), ctx))).toBe(false);
  });

  it("only warns about the risky choices", () => {
    const issues = validateAgent(valid({ mode: "yolo", sandbox: "off", tools: [], system_prompt: "x".repeat(7000) }), { ...ctx, otherNames: ["revisore"] });
    expect(hasErrors(issues)).toBe(false);
    expect(issues.map((i) => i.field).sort()).toEqual(["mode", "name", "sandbox", "system_prompt", "tools"]);
  });
});

describe("helpers", () => {
  it("estimates tokens like the backend (three characters each)", () => {
    expect(estimateTokens("")).toBe(0);
    expect(estimateTokens("abcd")).toBe(2);
  });

  it("finds the agents on a profile", () => {
    expect(agentsUsingProfile(agents, "balanced").map((a) => a.id)).toEqual(["operator", "coder"]);
    expect(agentsUsingProfile(agents, "nope")).toEqual([]);
  });
});

describe("response schema text", () => {
  it("is empty when there is no schema, and an object otherwise", () => {
    expect(parseSchemaText("  ")).toEqual({});
    expect(parseSchemaText('{"type":"object"}')).toEqual({ value: { type: "object" } });
    expect(schemaToText(undefined)).toBe("");
    expect(parseSchemaText(schemaToText({ type: "array" })).value).toEqual({ type: "array" });
  });
  it("refuses text that is not a JSON object", () => {
    expect(parseSchemaText("{type:").error).toMatch(/^JSON non valido/);
    for (const text of ["[]", '"x"', "3", "null"]) expect(parseSchemaText(text).error, text).toMatch(/oggetto JSON/);
  });
});

describe("run budget", () => {
  it("keeps only the limits that are set, and drops an empty budget", () => {
    expect(cleanBudget(undefined)).toBeUndefined();
    expect(cleanBudget({})).toBeUndefined();
    expect(cleanBudget({ max_tokens: Number.NaN, max_turns: 10 })).toEqual({ max_turns: 10 });
    expect(normalizeAgent(valid({ budget: { max_seconds: Number.NaN } })).budget).toBeUndefined();
    expect("budget" in normalizeAgent(valid({ budget: {} }))).toBe(false);
    expect(normalizeAgent(valid({ budget: { max_tokens: 5000 } })).budget).toEqual({ max_tokens: 5000 });
  });
  it("blocks saving on a limit the backend would refuse", () => {
    for (const bad of [{ max_turns: 0 }, { max_tokens: -5 }, { max_seconds: 1.5 }]) {
      expect(validateAgent(valid({ budget: bad }), ctx).filter((i) => i.field === "budget" && i.level === "error"), JSON.stringify(bad)).toHaveLength(1);
    }
    expect(validateAgent(valid({ budget: { max_tokens: 1, max_turns: 3, max_seconds: 60 } }), ctx).filter((i) => i.field === "budget")).toEqual([]);
    expect(validateAgent(valid(), ctx).filter((i) => i.field === "budget")).toEqual([]);
  });
});
