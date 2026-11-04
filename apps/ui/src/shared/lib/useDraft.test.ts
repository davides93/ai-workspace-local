import { describe, expect, it } from "vitest";
import { syncDraft, type DraftState } from "./useDraft";

const make = (v: string) => () => v.toUpperCase();
const state = (draft: string, base: string, scope = "a"): DraftState<string> => ({ draft, scope, base });

describe("syncDraft", () => {
  it("waits for the saved value", () => {
    expect(syncDraft(null, { savedKey: null, scope: "a", draftKey: null, make: make("x") })).toBeNull();
  });

  it("starts a draft from the first saved value", () => {
    expect(syncDraft(null, { savedKey: "one", scope: "a", draftKey: null, make: make("one") })).toEqual({ draft: "ONE", scope: "a", base: "one" });
  });

  it("starts over when the scope changes, even with edits pending", () => {
    const s = state("edited", "one", "a");
    expect(syncDraft(s, { savedKey: "two", scope: "b", draftKey: "edited", make: make("two") })).toEqual({ draft: "TWO", scope: "b", base: "two" });
  });

  it("keeps the same state while nothing changed", () => {
    const s = state("one", "one");
    expect(syncDraft(s, { savedKey: "one", scope: "a", draftKey: "one", make: make("one") })).toBe(s);
  });

  it("follows the server while the user has not edited", () => {
    const s = state("one", "one");
    expect(syncDraft(s, { savedKey: "two", scope: "a", draftKey: "one", make: make("two") })).toEqual({ draft: "TWO", scope: "a", base: "two" });
  });

  it("rebases on the answer to its own save without touching the draft", () => {
    const s = state("mine", "one");
    const out = syncDraft(s, { savedKey: "mine", scope: "a", draftKey: "mine", make: make("mine") });
    expect(out).toEqual({ draft: "mine", scope: "a", base: "mine" });
    expect(out?.draft).toBe("mine");
  });

  it("never overwrites edits with a different server version", () => {
    const s = state("mine", "one");
    expect(syncDraft(s, { savedKey: "theirs", scope: "a", draftKey: "mine", make: make("theirs") })).toBe(s);
  });
});
