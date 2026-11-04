import { describe, expect, it } from "vitest";
import { filterCatalog, filterInstalled, installedFormats, matchesQuery, normalize, originCounts, summarizeInstalled } from "./filters";
import { fitRank, isFit } from "./fit";
import { isExternalOrigin, originInfo } from "./origin";
import { catalogEntry, local } from "./testdata";

describe("text matching", () => {
  it("ignores case and accents and needs every word", () => {
    expect(normalize("Città È")).toBe("citta e");
    expect(matchesQuery("Qwen3 4B Instruct Q4_K_M", "qwen q4")).toBe(true);
    expect(matchesQuery("Qwen3 4B Instruct", "qwen llama")).toBe(false);
    expect(matchesQuery("anything", "   ")).toBe(true);
  });
});

describe("fit and origin", () => {
  it("ranks fits from best to worst and puts the unknown last", () => {
    expect(["no", "great", "tight", "ok", ""].sort((a, b) => fitRank(a) - fitRank(b))).toEqual(["great", "ok", "tight", "no", ""]);
    expect(isFit("tight")).toBe(true);
    expect(isFit("")).toBe(false);
  });

  it("knows which origins are read-only stores of other apps", () => {
    expect(isExternalOrigin("ollama")).toBe(true);
    expect(isExternalOrigin("lmstudio")).toBe(true);
    expect(isExternalOrigin("hf-cache")).toBe(true);
    expect(isExternalOrigin("aiw")).toBe(false);
    expect(isExternalOrigin("manual")).toBe(false);
    // an unknown or missing origin (older backend) is a plain file in the models folder
    expect(isExternalOrigin(undefined)).toBe(false);
    expect(originInfo("???").label).toBe("Manuale");
    // the backend sends "" for a model whose origin it does not know (older stores, files copied by hand)
    expect(originInfo("").label).toBe("Manuale");
    expect(isExternalOrigin("")).toBe(false);
  });
});

describe("a model with an empty origin", () => {
  const legacy = local({ id: "old", name: "Vecchio", origin: "", size_bytes: 100 });
  it("is read as a manual file: it matches the manual filter and is counted as manual", () => {
    expect(filterInstalled([legacy], { q: "", origin: "manual", format: "" })).toHaveLength(1);
    expect(filterInstalled([legacy], { q: "", origin: "aiw", format: "" })).toHaveLength(0);
    expect(originCounts([legacy])).toEqual({ manual: 1 });
  });
});

describe("installed models", () => {
  const list = [
    local({ id: "a", name: "Qwen3 4B", origin: "aiw", quant: "Q4_K_M", size_bytes: 3_000 }),
    local({ id: "ollama:llama3.2:3b", name: "Llama 3.2 3B", origin: "ollama", removable: false, size_bytes: 2_000 }),
    local({ id: "c", name: "Phi-4 mini", origin: "manual", format: "onnx", size_bytes: 500 }),
    local({ id: "hf-cache:Qwen/Qwen3-8B-GGUF/x.gguf", name: "Qwen3 8B", origin: "hf-cache", removable: false, size_bytes: 4_000, arch: "qwen3" }),
  ];

  it("filters by text, origin and format together", () => {
    expect(filterInstalled(list, { q: "", origin: "", format: "" })).toHaveLength(4);
    expect(filterInstalled(list, { q: "qwen", origin: "", format: "" }).map((m) => m.id)).toEqual(["a", "hf-cache:Qwen/Qwen3-8B-GGUF/x.gguf"]);
    expect(filterInstalled(list, { q: "", origin: "ollama", format: "" }).map((m) => m.name)).toEqual(["Llama 3.2 3B"]);
    expect(filterInstalled(list, { q: "", origin: "", format: "onnx" }).map((m) => m.id)).toEqual(["c"]);
    expect(filterInstalled(list, { q: "qwen", origin: "aiw", format: "gguf" }).map((m) => m.id)).toEqual(["a"]);
    expect(filterInstalled(list, { q: "zzz", origin: "", format: "" })).toEqual([]);
  });

  it("matches the quantization and the architecture too", () => {
    expect(filterInstalled(list, { q: "q4_k_m", origin: "", format: "" }).map((m) => m.id)).toEqual(["a"]);
    expect(filterInstalled(list, { q: "qwen3 arch", origin: "", format: "" })).toEqual([]);
    expect(filterInstalled(list, { q: "qwen3", origin: "hf-cache", format: "" })).toHaveLength(1);
  });

  it("counts origins, lists formats and sums what AIW owns apart from what it only reads", () => {
    expect(originCounts(list)).toEqual({ aiw: 1, ollama: 1, manual: 1, "hf-cache": 1 });
    expect(installedFormats(list)).toEqual(["gguf", "onnx"]);
    expect(summarizeInstalled(list)).toEqual({ total: 4, ownBytes: 3_500, externalBytes: 6_000, externalCount: 2 });
  });
});

describe("catalog", () => {
  const list = [
    catalogEntry({ id: "tight", name: "Grande", fit: "tight" }),
    catalogEntry({ id: "no", name: "Enorme", fit: "no" }),
    catalogEntry({ id: "great-1", name: "Piccolo", fit: "great", tags: ["mobile"] }),
    catalogEntry({ id: "ok", name: "Medio", fit: "ok" }),
    catalogEntry({ id: "great-2", name: "Visione", fit: "great", capabilities: ["vision"] }),
  ];

  it("puts what fits best first and keeps the curated order inside a tier", () => {
    expect(filterCatalog(list, { q: "", fitOnly: false }).map((c) => c.id)).toEqual(["great-1", "great-2", "ok", "tight", "no"]);
  });

  it("hides what does not fit on request and searches name, tags and capabilities", () => {
    expect(filterCatalog(list, { q: "", fitOnly: true }).map((c) => c.id)).not.toContain("no");
    expect(filterCatalog(list, { q: "mobile", fitOnly: false }).map((c) => c.id)).toEqual(["great-1"]);
    expect(filterCatalog(list, { q: "vision", fitOnly: false }).map((c) => c.id)).toEqual(["great-2"]);
  });
});
