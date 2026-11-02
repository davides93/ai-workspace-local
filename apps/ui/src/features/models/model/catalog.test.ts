import { describe, expect, it } from "vitest";
import { catalogSize, catalogState, catalogTags, findCatalogJob, isWholeRepo, platformNote, systemNote, systemRuntime } from "./catalog";
import { catalogEntry, job } from "./testdata";

const mlx = catalogEntry({
  id: "qwen3-4b-mlx", name: "Qwen3 4B (MLX)", format: "mlx", quant: "4bit", size_bytes: 2_300_000_000,
  source: { kind: "hf", repo: "mlx-community/Qwen3-4B-4bit", file: "" }, fit: "no",
});
const system = catalogEntry({ id: "apple-fm-system", format: "system", quant: "2-bit QAT", size_bytes: 0, source: { kind: "system", runtime: "apple-fm" }, downloadable: false });

describe("catalog entries", () => {
  it("recognizes the MLX entry as a whole repository", () => {
    expect(isWholeRepo(mlx)).toBe(true);
    expect(isWholeRepo(catalogEntry())).toBe(false);
    expect(isWholeRepo(system)).toBe(false);
    expect(catalogTags(mlx)).toEqual([".mlx", "4bit", "repository intero"]);
    expect(catalogTags(catalogEntry())).toEqual([".gguf", "Q4_K_M"]);
  });

  it("says the MLX platform requirement out loud", () => {
    expect(platformNote(mlx)).toMatch(/macOS con Apple Silicon/);
    expect(platformNote(catalogEntry())).toBeNull();
    expect(platformNote(system)).toBeNull();
  });

  it("has a size to download only when there is something to download", () => {
    expect(catalogSize(catalogEntry())).toMatch(/GB/);
    expect(catalogSize(system)).toBeNull();
  });

  it("explains where a system model comes from", () => {
    expect(systemRuntime(system)).toBe("apple-fm");
    expect(systemRuntime(catalogEntry())).toBeNull();
    expect(systemNote("apple-fm")).toMatch(/Apple Intelligence/);
    expect(systemNote("gemini-nano")).toMatch(/Android/);
    expect(systemNote("phi-silica")).toMatch(/Copilot\+/);
    expect(systemNote(null)).toMatch(/non si scarica/);
  });
});

describe("catalogState", () => {
  const running = job({ model_id: "qwen3-4b", status: "running" });

  it("gives a system model no download at all", () => {
    expect(catalogState(system, undefined)).toBe("system");
    expect(catalogState({ ...system, installed: true }, running)).toBe("system");
  });
  it("prefers installed, then in progress, then blocked, then ready", () => {
    expect(catalogState(catalogEntry({ installed: true }), running)).toBe("installed");
    expect(catalogState(catalogEntry(), running)).toBe("downloading");
    expect(catalogState(catalogEntry({ fit: "no" }), undefined)).toBe("blocked");
    expect(catalogState(mlx, undefined)).toBe("blocked");
    expect(catalogState(catalogEntry({ fit: "tight" }), undefined)).toBe("ready");
  });
});

describe("findCatalogJob", () => {
  it("ties a download to its entry through the catalog id kept as model_id", () => {
    const jobs = [job({ id: "a", model_id: "qwen3-4b", status: "done" }), job({ id: "b", model_id: "qwen3-4b", status: "running" }), job({ id: "c", model_id: "other", status: "running" })];
    expect(findCatalogJob(jobs, { id: "qwen3-4b" })?.id).toBe("b");
    expect(findCatalogJob(jobs, { id: "none" })).toBeUndefined();
  });
});
