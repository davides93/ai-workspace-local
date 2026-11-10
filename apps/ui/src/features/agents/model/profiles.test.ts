import { describe, expect, it } from "vitest";
import { models, profiles, runtimes } from "../../../shared/api/mock/fixtures";
import type { InferenceProfile, LocalModel } from "../../../shared/api/types";
import {
  blankProfile, chatRuntimes, compatibleModels, hasProfileErrors, hiddenProfileErrors, idForProfile, incompatibility, modelAfterRuntimeChange, normalizeProfile, validateProfile,
} from "./profiles";

const model = (id: string, compat: { id: string; available: boolean; reason?: string }[], over: Partial<LocalModel> = {}): LocalModel => ({
  id, name: id, format: "gguf", size_bytes: 1, capabilities: [], runtimes: compat.map((c) => c.id), origin: "aiw", removable: true, fit: "ok", fit_detail: "",
  runtime_compat: compat.map((c) => ({ id: c.id, name: c.id, available: c.available, reason: c.reason ?? "" })), ...over,
});

const a = model("a", [{ id: "llama-server", available: true }, { id: "ollama", available: false, reason: "da installare" }]);
const b = model("b", [{ id: "mlx", available: true }]);
const ctx = { runtimes: runtimes, models: [a, b] };
const ok = (over: Partial<InferenceProfile> = {}): InferenceProfile => ({ ...blankProfile("llama-server", "a"), name: "Veloce", ...over });

describe("model compatibility", () => {
  it("keeps the models a runtime can open", () => {
    expect(compatibleModels([a, b], "llama-server").map((m) => m.id)).toEqual(["a"]);
    expect(compatibleModels([a, b], "mlx").map((m) => m.id)).toEqual(["b"]);
    expect(compatibleModels([a, b], "nothing")).toEqual([]);
  });

  it("never offers speech models or speech runtimes to a chat profile", () => {
    const whisper = model("ggml-small-q8_0", [{ id: "whisper-server", available: true }, { id: "llama-server", available: true }], { format: "ggml", capabilities: ["stt"] });
    expect(compatibleModels([a, whisper], "llama-server").map((m) => m.id)).toEqual(["a"]);
    expect(compatibleModels([whisper], "whisper-server")).toEqual([]);
    expect(chatRuntimes(runtimes).some((r) => r.id === "whisper-server")).toBe(false);
    expect(chatRuntimes(runtimes).some((r) => r.id === "llama-server")).toBe(true);
    expect(chatRuntimes([{ capabilities: ["stt", "chat"] }, { capabilities: [] }])).toHaveLength(2);
  });

  it("works with the demo catalogue", () => {
    expect(compatibleModels(models, "llama-server").length).toBeGreaterThan(0);
  });

  it("explains why a model does not fit", () => {
    expect(incompatibility(a, "llama-server")).toBe("");
    expect(incompatibility(a, "ollama")).toBe("da installare");
    expect(incompatibility(b, "llama-server")).toMatch(/formato/);
    expect(incompatibility(undefined, "x")).toBe("");
  });

  it("picks a model after the runtime changed", () => {
    expect(modelAfterRuntimeChange("a", [a, b], "llama-server")).toBe("a");
    expect(modelAfterRuntimeChange("a", [a, b], "mlx")).toBe("b");
    expect(modelAfterRuntimeChange("a", [a, b], "nothing")).toBe("");
  });
});

describe("ids and normalization", () => {
  it("derives unique ids", () => {
    expect(idForProfile("Bilanciato (GPU)", profiles)).toBe("bilanciato-gpu");
    expect(idForProfile("Veloce", [{ ...ok(), id: "veloce" }])).toBe("veloce-2");
    expect(idForProfile("!!!", [])).toBe("profilo");
  });

  it("leaves empty parameters out and rounds the integers", () => {
    const n = normalizeProfile(ok({ name: " Veloce ", description: "  ", params: { temperature: 0.4, top_p: undefined, max_tokens: 1024.4, context: Number.NaN } }));
    expect(n.name).toBe("Veloce");
    expect(n.description).toBeUndefined();
    expect(n.params).toEqual({ temperature: 0.4, max_tokens: 1024 });
  });

  it("keeps top_p of a profile that has it", () => {
    expect(normalizeProfile(ok({ params: { top_p: 0.95 } })).params).toEqual({ top_p: 0.95 });
  });

  it("keeps the adapters with a path and drops the empty ones", () => {
    const n = normalizeProfile(ok({ params: { adapters: [{ path: " /a/lora.gguf ", scale: 0.5 }, { path: "  " }, { path: "/a/b.gguf", scale: Number.NaN }] } }));
    expect(n.params.adapters).toEqual([{ path: "/a/lora.gguf", scale: 0.5 }, { path: "/a/b.gguf" }]);
    expect(normalizeProfile(ok({ params: { adapters: [{ path: "" }] } })).params.adapters).toBeUndefined();
  });
});

describe("adapters in a profile", () => {
  it("are refused for a runtime that cannot load them, and for a bad scale", () => {
    const withAdapter = (runtime_id: string, scale?: number) => ok({ runtime_id, params: { adapters: [{ path: "/a/lora", scale }] } });
    expect(validateProfile(withAdapter("llama-server"), ctx).filter((i) => i.field === "adapters")).toEqual([]);
    const refused = validateProfile(withAdapter("ollama"), ctx).find((i) => i.field === "adapters");
    expect(refused).toMatchObject({ level: "error", message: expect.stringContaining("non carica adapter") });
    expect(hasProfileErrors(validateProfile(withAdapter("llama-server", 0), ctx))).toBe(true);
    // No adapter, no complaint, whatever the runtime.
    expect(validateProfile(ok({ runtime_id: "ollama" }), ctx).some((i) => i.field === "adapters")).toBe(false);
  });
});

describe("validateProfile", () => {
  it("accepts a sound profile", () => {
    expect(validateProfile(ok(), ctx)).toEqual([]);
  });

  it("needs a name, a runtime and a model", () => {
    const issues = validateProfile(ok({ name: "", runtime_id: "", model: "" }), ctx);
    expect(issues.filter((i) => i.level === "error").map((i) => i.field).sort()).toEqual(["model", "name", "runtime_id"]);
    expect(hasProfileErrors(issues)).toBe(true);
  });

  it("checks the ranges of the parameters", () => {
    for (const params of [{ temperature: 2.5 }, { temperature: -1 }, { top_p: 0 }, { top_p: 1.5 }, { context: 100 }, { max_tokens: 0 }]) {
      expect(hasProfileErrors(validateProfile(ok({ params }), ctx)), JSON.stringify(params)).toBe(true);
    }
    expect(hasProfileErrors(validateProfile(ok({ params: { temperature: 2, top_p: 1, context: 512, max_tokens: 1 } }), ctx))).toBe(false);
  });

  it("warns, without blocking, about an answer as long as the context and a model that does not fit", () => {
    const long = validateProfile(ok({ params: { context: 4096, max_tokens: 4096 } }), ctx);
    expect(long.map((i) => [i.field, i.level])).toEqual([["max_tokens", "warn"]]);
    const wrong = validateProfile(ok({ runtime_id: "ollama" }), ctx);
    expect(wrong).toEqual([expect.objectContaining({ field: "model", level: "warn" })]);
    const ctxModel = { ...ctx, models: [model("a", [{ id: "llama-server", available: true }], { context_length: 8192 })] };
    expect(validateProfile(ok({ params: { context: 16384 } }), ctxModel).map((i) => i.field)).toEqual(["context"]);
  });
});

describe("what saving the simple form keeps", () => {
  it("keeps the numbers, the adapters and the reasoning already stored", () => {
    const stored = ok({ params: { temperature: 0.45, top_p: 0.8, context: 16384, max_tokens: 3000, thinking: "deep", thinking_budget: 4096, adapters: [{ path: "/a/lora.gguf", scale: 0.5 }] } });
    expect(normalizeProfile(stored).params).toEqual(stored.params);
  });
  it("drops a reasoning value that is not one of the three and a budget that is not a number", () => {
    const n = normalizeProfile(ok({ params: { thinking: "tanto" as never, thinking_budget: Number.NaN } }));
    expect(n.params).toEqual({});
  });
  it("checks the reasoning budget", () => {
    expect(validateProfile(ok({ params: { thinking_budget: 0 } }), ctx).map((i) => i.field)).toEqual(["thinking_budget"]);
    expect(hasProfileErrors(validateProfile(ok({ params: { thinking_budget: 2048 } }), ctx))).toBe(false);
  });
  it("tells errors in fields the simple form hides from the ones it shows", () => {
    const issues = validateProfile(ok({ name: "", params: { context: 100 } }), ctx);
    expect(hiddenProfileErrors(issues).map((i) => i.field)).toEqual(["context"]);
  });
});
