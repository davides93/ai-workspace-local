import { describe, expect, it } from "vitest";
import type { InferenceProfile, LocalModel } from "../../../shared/api/types";
import { assistantModelName, SIMPLE_ACTIONS, simpleSections } from "./simple";

const model = (id: string, name: string) => ({ id, name }) as LocalModel;
const profile = (m: string) => ({ id: "p", model: m }) as InferenceProfile;

describe("simple Panoramica", () => {
  it("offers only a new chat and a model download", () => {
    expect(SIMPLE_ACTIONS.map((a) => a.id)).toEqual(["new-chat", "download-model"]);
    expect(SIMPLE_ACTIONS.map((a) => a.title)).toEqual(["Nuova chat", "Scarica un modello"]);
  });
  it("names the model in use with its human name, never the id", () => {
    expect(assistantModelName(profile("qwen3-4b-q4"), [model("qwen3-4b-q4", "Qwen3 4B")])).toBe("Qwen3 4B");
  });
  it("has no name without a profile, a chosen model or a model on disk", () => {
    expect(assistantModelName(undefined, [model("a", "A")])).toBeUndefined();
    expect(assistantModelName(profile(""), [model("a", "A")])).toBeUndefined();
    expect(assistantModelName(profile("b"), [model("a", "A")])).toBeUndefined();
    expect(assistantModelName(profile("a"), [model("a", "  ")])).toBeUndefined();
  });
  it("shows only the blocks that have something to say", () => {
    expect(simpleSections({ approvals: 0, sessions: 0, lessons: 0 })).toEqual({ assistant: false, approvals: false, recent: false, lessons: false });
    expect(simpleSections({ modelName: "Qwen3 4B", approvals: 2, sessions: 3, lessons: 1 })).toEqual({ assistant: true, approvals: true, recent: true, lessons: true });
  });
});
