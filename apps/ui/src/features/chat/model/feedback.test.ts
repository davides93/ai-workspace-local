import { describe, expect, it } from "vitest";
import type { Feedback } from "../../../shared/api/types";
import { finalAnswerIds, ratingChange, ratingsById, withoutRating, withRating } from "./feedback";
import type { Item } from "./items";

const user = (id: string): Item => ({ kind: "user", id, text: id });
const text = (id: string, streaming = false, t = "risposta"): Item => ({ kind: "text", id, text: t, streaming });
const tool = (id: string): Item => ({ kind: "tool", id, name: "shell", args: "{}", status: "done" });
const fb = (message_id: string, rating: 1 | -1, note?: string): Feedback => ({ id: `f_${message_id}`, session_id: "s", message_id, rating, note, created_at: "t" });

describe("finalAnswerIds", () => {
  it("picks the last text of each turn", () => {
    const items = [user("u1"), text("a1"), user("u2"), text("a2")];
    expect([...finalAnswerIds(items, false)]).toEqual(["a1", "a2"]);
  });
  it("skips the text that announces a tool call and every text followed by work", () => {
    const items = [user("u1"), text("a1", false, "Guardo la cartella."), tool("c1"), text("a2", false, "Ho finito."), user("u2"), text("a3"), tool("c2")];
    expect([...finalAnswerIds(items, false)]).toEqual(["a2"]);
  });
  it("takes the answer after a self-check note, not the one before it", () => {
    const items: Item[] = [user("u1"), text("a1"), { kind: "note", id: "n", text: "Controllo automatico" }, text("a2")];
    expect([...finalAnswerIds(items, false)]).toEqual(["a2"]);
  });
  it("leaves out the turn that is still going on, and text still streaming", () => {
    const items = [user("u1"), text("a1"), user("u2"), text("a2")];
    expect([...finalAnswerIds(items, true)]).toEqual(["a1"]);
    expect([...finalAnswerIds([user("u"), text("a", true)], false)]).toEqual([]);
    expect([...finalAnswerIds([user("u"), text("a", false, "  ")], false)]).toEqual([]);
  });
  it("ignores reasoning and errors, and rates A2UI actions as turn boundaries", () => {
    const items: Item[] = [user("u"), text("a1"), { kind: "reasoning", id: "r", text: "…", streaming: false }, { kind: "action", id: "x", name: "ok", context: {} }, text("a2")];
    expect([...finalAnswerIds(items, false)]).toEqual(["a1", "a2"]);
  });
});

describe("ratings", () => {
  it("takes the rating back when the same thumb is pressed again", () => {
    expect(ratingChange(undefined, 1)).toEqual({ kind: "set", rating: 1 });
    expect(ratingChange(fb("m", 1), 1)).toEqual({ kind: "clear" });
    expect(ratingChange(fb("m", 1), -1)).toEqual({ kind: "set", rating: -1 });
    expect(ratingChange(fb("m", -1), -1)).toEqual({ kind: "clear" });
  });
  it("keeps one rating per message", () => {
    const list = withRating(withRating(undefined, fb("a", 1)), fb("b", -1));
    expect([...ratingsById(withRating(list, fb("a", -1, "no"))).values()].map((f) => [f.message_id, f.rating])).toEqual([["b", -1], ["a", -1]]);
    expect(withoutRating(list, "a").map((f) => f.message_id)).toEqual(["b"]);
    expect(withoutRating(undefined, "a")).toEqual([]);
  });
});
