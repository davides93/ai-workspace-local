import { describe, expect, it } from "vitest";
import { findInstalled, findSystem, galleryModels, isGalleryModel } from "./gallery";
import { catalogEntry, local } from "./testdata";

const apple = catalogEntry({ id: "apple-fm-system", name: "Apple", params_b: 3, format: "system", downloadable: false, source: { kind: "system", runtime: "apple-fm" }, tags: ["system", "mobile"] });
const phi = catalogEntry({ id: "phi-silica-system", params_b: 3.3, format: "system", downloadable: false, source: { kind: "system", runtime: "phi-silica" }, tags: ["system"] });
const small = catalogEntry({ id: "gemma-3-1b", name: "Gemma 3 1B", params_b: 1, tags: ["mobile", "fast"], source: { kind: "hf", repo: "unsloth/gemma-3-1b-it-GGUF", file: "gemma-3-1b-it-Q4_K_M.gguf" } });
const big = catalogEntry({ id: "qwen3-4b", tags: ["agent", "laptop"] });
const embed = catalogEntry({ id: "emb", tags: ["embeddings", "mobile"] });

describe("gallery", () => {
  it("keeps system models of phones and tablets and the mobile entries", () => {
    expect(isGalleryModel(apple)).toBe(true);
    expect(isGalleryModel(phi)).toBe(false);
    expect(isGalleryModel(small)).toBe(true);
    expect(isGalleryModel(big)).toBe(false);
    expect(isGalleryModel(embed)).toBe(false);
  });
  it("lists system models first, then by size", () => {
    const mid = catalogEntry({ id: "mid", name: "Mid", params_b: 3.2, tags: ["mobile"] });
    expect(galleryModels([mid, small, apple, big]).map((c) => c.id)).toEqual(["apple-fm-system", "gemma-3-1b", "mid"]);
  });
  it("finds the downloaded file of an entry", () => {
    const m = local({ id: "x", path: "/data/models/hf/unsloth/gemma-3-1b-it-GGUF/gemma-3-1b-it-Q4_K_M.gguf" });
    expect(findInstalled(small, [local(), m])).toBe(m);
    expect(findInstalled(small, [local({ path: "/a/other.gguf" })])).toBeUndefined();
    expect(findInstalled(apple, [m])).toBeUndefined();
  });
  it("finds the system model by id", () => {
    const m = local({ id: "apple-fm-system", origin: "system" });
    expect(findSystem(apple, [m])).toBe(m);
    expect(findSystem(small, [m])).toBeUndefined();
  });
});
