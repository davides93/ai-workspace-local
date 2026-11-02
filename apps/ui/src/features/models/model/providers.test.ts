import { describe, expect, it } from "vitest";
import type { ModelProviderInfo } from "../../../shared/api/types";
import { authBadge, discoverProviders, exactRef, formatOptions, hasSettings, parseSort, pickProvider, providerName, providerNote, pullRevision, sortOptionsFor, stubModel } from "./providers";

const info = (over: Partial<ModelProviderInfo>): ModelProviderInfo => ({
  id: "hf", name: "Hugging Face", description: "", homepage: "", capabilities: [], formats: ["gguf", "mlx"], auth: "optional", auth_configured: false, endpoint: "https://huggingface.co", ...over,
});

describe("providers", () => {
  it("leaves the curated catalog to its own tab", () => {
    expect(discoverProviders([info({ id: "catalog" }), info({ id: "hf" }), info({ id: "url" })]).map((p) => p.id)).toEqual(["hf", "url"]);
  });

  it("picks the provider named in the URL, else Hugging Face, else the first one", () => {
    const offered = [{ id: "modelscope" }, { id: "hf" }, { id: "ollama" }];
    expect(pickProvider(offered, "ollama")).toBe("ollama");
    expect(pickProvider(offered, "nope")).toBe("hf");
    expect(pickProvider(offered, undefined)).toBe("hf");
    expect(pickProvider([{ id: "ollama" }], undefined)).toBe("ollama");
    expect(pickProvider([], undefined)).toBe("hf");
  });

  it("names a provider from the backend list or from its id", () => {
    expect(providerName("hf", [{ id: "hf", name: "HF (mirror)" }])).toBe("HF (mirror)");
    expect(providerName("ollama")).toBe("Registry Ollama");
    expect(providerName("boh")).toBe("boh");
  });

  it("explains how to reach a model the search cannot list (Ollama, ModelScope)", () => {
    expect(providerNote("ollama")).toMatch(/nome esatto/);
    expect(providerNote("ollama")).toMatch(/qwen3:4b/);
    expect(providerNote("modelscope")).toMatch(/proprietario\/nome/);
    expect(providerNote("hf")).toBeNull();
  });
});

describe("token state", () => {
  it("shows set, optional, required and not-used", () => {
    expect(authBadge({ auth: "optional", auth_configured: true })).toMatchObject({ label: "Token impostato", tone: "ok" });
    expect(authBadge({ auth: "optional", auth_configured: false })).toMatchObject({ label: "Token opzionale", tone: "neutral" });
    expect(authBadge({ auth: "required", auth_configured: false })).toMatchObject({ label: "Token richiesto", tone: "warn" });
    expect(authBadge({ auth: "required", auth_configured: true }).label).toBe("Token impostato");
    expect(authBadge({ auth: "none", auth_configured: false }).label).toBe("Nessun token");
  });

  it("offers settings when there is a token or an endpoint to change", () => {
    expect(hasSettings({ auth: "optional", endpoint: "https://huggingface.co" })).toBe(true);
    expect(hasSettings({ auth: "none", endpoint: "https://registry.ollama.ai" })).toBe(true);
    expect(hasSettings({ auth: "none", endpoint: "" })).toBe(false);
  });
});

describe("search helpers", () => {
  it("uses the provider's own formats, without the pseudo format 'system'", () => {
    expect(formatOptions(info({ formats: ["gguf", "mlx", "system"] }))).toEqual(["gguf", "mlx"]);
    expect(formatOptions(undefined)).toContain("gguf");
    expect(formatOptions(info({ formats: [] }))).toContain("safetensors");
  });

  it("recognizes an exact model reference per provider", () => {
    expect(exactRef("hf", "Qwen/Qwen3-4B-GGUF")).toBe("Qwen/Qwen3-4B-GGUF");
    expect(exactRef("hf", "  unsloth/gemma-3n-E4B-it-GGUF ")).toBe("unsloth/gemma-3n-E4B-it-GGUF");
    expect(exactRef("hf", "qwen")).toBeNull();
    expect(exactRef("hf", "a b/c")).toBeNull();
    expect(exactRef("modelscope", "Qwen/Qwen3-8B-GGUF")).toBe("Qwen/Qwen3-8B-GGUF");
    expect(exactRef("ollama", "qwen3:4b")).toBe("qwen3:4b");
    expect(exactRef("ollama", "qwen3")).toBe("qwen3");
    expect(exactRef("ollama", "user/model:latest")).toBe("user/model:latest");
    expect(exactRef("ollama", "two words")).toBeNull();
    expect(exactRef("url", "https://x.org/a.gguf")).toBeNull();
    expect(exactRef("hf", "")).toBeNull();
  });

  it("builds a stub record for a model that is not in the results", () => {
    expect(stubModel("hf", "Qwen/Qwen3-4B-GGUF")).toMatchObject({ provider: "hf", id: "Qwen/Qwen3-4B-GGUF", name: "Qwen3-4B-GGUF", publisher: "Qwen", gated: false });
    expect(stubModel("ollama", "qwen3:4b")).toMatchObject({ name: "qwen3:4b", publisher: "library" });
  });

  it("pins the revision only on the repository hubs", () => {
    expect(pullRevision("hf", "abc123")).toBe("abc123");
    expect(pullRevision("modelscope", "master")).toBe("master");
    expect(pullRevision("ollama", "latest")).toBeUndefined();
    expect(pullRevision("hf", undefined)).toBeUndefined();
  });

  it("parses the sort key and hides 'likes' on Ollama, which has none", () => {
    expect(parseSort("likes")).toBe("likes");
    expect(parseSort("boh")).toBe("downloads");
    expect(parseSort(undefined)).toBe("downloads");
    expect(sortOptionsFor("hf").map((o) => o.id)).toEqual(["downloads", "likes", "updated"]);
    expect(sortOptionsFor("ollama").map((o) => o.id)).toEqual(["downloads", "updated"]);
  });
});
