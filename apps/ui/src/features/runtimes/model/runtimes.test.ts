import { describe, expect, it } from "vitest";
import type { LocalModel, RuntimeInfo, SettingsSchema } from "../../../shared/api/types";
import { apiLabel, compatibleModels, defaultSettings, filterRuntimes, groupByKind, logTail, matchesRuntime, runtimeCounts, settingsDirty } from "./runtimes";

const rt = (id: string, over: Partial<RuntimeInfo> = {}): RuntimeInfo => ({
  id, name: id, kind: "sidecar", description: `runtime ${id}`, platforms: ["linux"], accelerators: ["cpu"], formats: ["gguf"], api: "openai",
  capabilities: ["chat"], status: "available", loaded_models: [], settings_schema: { type: "object", properties: {} }, settings: {}, ...over,
});
const model = (id: string, compat: [string, boolean][], over: Partial<LocalModel> = {}): LocalModel => ({
  id, name: id, format: "gguf", size_bytes: 1, capabilities: [], runtimes: compat.map(([c]) => c), origin: "aiw", removable: true,
  runtime_compat: compat.map(([c, available]) => ({ id: c, name: c, available, reason: "" })), fit: "great", fit_detail: "", ...over,
});

const LIST = [
  rt("llama-server", { status: "running", vendor: "ggml-org", accelerators: ["cuda", "cpu"] }),
  rt("llama-embedded", { kind: "embedded" }),
  rt("vllm", { status: "installable", formats: ["safetensors"] }),
  rt("mlx-lm", { status: "unsupported", accelerators: ["metal"], formats: ["mlx"] }),
  rt("remote-mac", { kind: "remote", status: "running" }),
];

describe("filterRuntimes", () => {
  it("hides unsupported runtimes unless the whole catalog is asked for", () => {
    expect(filterRuntimes(LIST, { scope: "here", query: "" }).map((r) => r.id)).not.toContain("mlx-lm");
    expect(filterRuntimes(LIST, { scope: "all", query: "" })).toHaveLength(5);
  });
  it("searches name, vendor, accelerators and formats", () => {
    expect(matchesRuntime(LIST[0], "GGML")).toBe(true);
    expect(matchesRuntime(LIST[0], "cuda")).toBe(true);
    expect(matchesRuntime(LIST[3], "mlx")).toBe(true);
    expect(matchesRuntime(LIST[1], "cuda")).toBe(false);
    expect(filterRuntimes(LIST, { scope: "all", query: "safetensors" }).map((r) => r.id)).toEqual(["vllm"]);
  });
});

describe("groupByKind", () => {
  it("keeps the display order and drops empty kinds", () => {
    expect(groupByKind(LIST).map((g) => g.kind)).toEqual(["sidecar", "embedded", "remote"]);
    expect(groupByKind([])).toEqual([]);
  });
});

describe("runtimeCounts", () => {
  it("counts running, usable here and total", () => {
    expect(runtimeCounts(LIST)).toEqual({ running: 2, usable: 4, total: 5 });
  });
});

describe("compatibleModels", () => {
  const models = [
    model("tight-one", [["llama-server", true]], { fit: "tight", name: "B" }),
    model("great-one", [["llama-server", true], ["ollama", false]], { fit: "great", name: "A" }),
    model("not-here", [["llama-server", false]]),
    model("onnx", [["foundry", true]]),
    model("legacy", [], { runtimes: ["llama-server"], runtime_compat: [], fit: "" }),
  ];
  it("keeps models whose compatibility entry is available, best fit first", () => {
    expect(compatibleModels(models, "llama-server").map((m) => m.id)).toEqual(["great-one", "tight-one", "legacy"]);
    expect(compatibleModels(models, "ollama")).toEqual([]);
    expect(compatibleModels(models, "foundry").map((m) => m.id)).toEqual(["onnx"]);
  });
});

describe("settings", () => {
  const schema: SettingsSchema = {
    type: "object",
    properties: { ctx: { type: "integer", default: 8192 }, flash: { type: "boolean", default: true }, extra: { type: "array", default: [] }, name: { type: "string" } },
  };
  it("collects the defaults of the schema", () => {
    expect(defaultSettings(schema)).toEqual({ ctx: 8192, flash: true, extra: [] });
  });
  it("does not count an untouched default as a change", () => {
    expect(settingsDirty(schema, {}, { ctx: 8192 })).toBe(false);
    expect(settingsDirty(schema, { ctx: 4096 }, { ctx: 4096, flash: true })).toBe(false);
  });
  it("sees a real change, whatever the key order", () => {
    expect(settingsDirty(schema, { ctx: 4096, flash: false }, { flash: false, ctx: 4097 })).toBe(true);
    expect(settingsDirty(schema, {}, { extra: ["--x"] })).toBe(true);
    expect(settingsDirty(schema, { flash: false, ctx: 1 }, { ctx: 1, flash: false })).toBe(false);
  });
});

describe("logTail", () => {
  it("keeps the last lines", () => {
    expect(logTail(["a", "b", "c"], 2)).toEqual(["b", "c"]);
    expect(logTail(["a"], 2)).toEqual(["a"]);
  });
});

describe("apiLabel", () => {
  it("names the API of a runtime, and says so when there is none", () => {
    expect(apiLabel("openai")).toBe("API openai");
    expect(apiLabel("native")).toBe("API native");
    expect(apiLabel("none")).toBe("senza API");
  });
});
