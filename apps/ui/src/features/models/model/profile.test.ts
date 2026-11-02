import { describe, expect, it } from "vitest";
import { applyModelToProfile, newProfileFor, pickRuntime, profilesUsingModel, runtimeOptions, slugify, uniqueId } from "./profile";
import { local, profile, rt } from "./testdata";

describe("runtimes for a model", () => {
  const m = local({ runtime_compat: [rt("a", false, "da installare"), rt("b"), rt("c", false, "solo macOS"), rt("d")] });

  it("lists the available runtimes first", () => {
    expect(runtimeOptions(m).map((r) => r.id)).toEqual(["b", "d", "a", "c"]);
  });

  it("keeps the runtime the profile already uses when it can run the model", () => {
    expect(pickRuntime(m, "d")).toBe("d");
  });

  it("falls back to the first available runtime, never to an unavailable one", () => {
    expect(pickRuntime(m, "a")).toBe("b");
    expect(pickRuntime(m, "zzz")).toBe("b");
    expect(pickRuntime(m)).toBe("b");
  });

  it("is null when nothing can run the model here", () => {
    expect(pickRuntime(local({ runtime_compat: [rt("a", false)] }), "a")).toBeNull();
    expect(pickRuntime(local({ runtime_compat: [] }))).toBeNull();
  });
});

describe("profiles", () => {
  it("finds the profiles that run a model", () => {
    const list = [profile({ id: "a", model: "m1" }), profile({ id: "b", model: "m2" }), profile({ id: "c", model: "m1" })];
    expect(profilesUsingModel(list, "m1").map((p) => p.id)).toEqual(["a", "c"]);
    expect(profilesUsingModel(list, "zz")).toEqual([]);
  });

  it("swaps model and runtime and leaves everything else of the profile alone", () => {
    const p = profile({ name: "Veloce", description: "x", params: { temperature: 0.4, context: 4096 } });
    const next = applyModelToProfile(p, { id: "m2", context_length: 131072 }, "ollama");
    expect(next).toEqual({ ...p, model: "m2", runtime_id: "ollama" });
    expect(p.model).toBe("m1");
  });

  it("brings a context larger than the model's window back to it", () => {
    const p = profile({ params: { context: 65536, temperature: 0.2 } });
    expect(applyModelToProfile(p, { id: "m2", context_length: 32768 }, "r").params).toEqual({ context: 32768, temperature: 0.2 });
    expect(applyModelToProfile(p, { id: "m2" }, "r").params.context).toBe(65536);
  });

  it("slugifies names and keeps ids unique", () => {
    expect(slugify("Qwen3.5 · Veloce!")).toBe("qwen3-5-veloce");
    expect(slugify("Città d'arte")).toBe("citta-d-arte");
    expect(slugify("···")).toBe("profilo");
    expect(slugify("x".repeat(80)).length).toBeLessThanOrEqual(40);
    expect(uniqueId("a", ["b"])).toBe("a");
    expect(uniqueId("a", ["a", "a-2"])).toBe("a-3");
  });

  it("builds a new profile with moderate defaults inside the model's window", () => {
    const p = newProfileFor({ id: "m9", name: "Gemma 4", context_length: 131072 }, "llama-server", "  Mio profilo ", ["mio-profilo"]);
    expect(p).toMatchObject({ id: "mio-profilo-2", name: "Mio profilo", runtime_id: "llama-server", model: "m9" });
    expect(p.params.context).toBe(32768);
    expect(newProfileFor({ id: "m", name: "Piccolo", context_length: 4096 }, "r", "x", []).params.context).toBe(4096);
    expect(newProfileFor({ id: "m", name: "Senza", context_length: undefined }, "r", "x", []).params.context).toBe(8192);
  });
});
