import { describe, expect, it } from "vitest";
import type { Item } from "./items";
import { distanceFromBottom, isNearBottom, tailSignature } from "./scroll";

const text = (id: string, t: string, streaming = false): Item => ({ kind: "text", id, text: t, streaming });
const user = (id: string): Item => ({ kind: "user", id, text: "ciao" });

describe("scroll position", () => {
  it("measures what is hidden below and takes 64 px as «at the end»", () => {
    expect(distanceFromBottom({ scrollTop: 100, scrollHeight: 1000, clientHeight: 600 })).toBe(300);
    expect(distanceFromBottom({ scrollTop: 500, scrollHeight: 1000, clientHeight: 600 })).toBe(0);
    expect(isNearBottom({ scrollTop: 340, scrollHeight: 1000, clientHeight: 600 })).toBe(true);
    expect(isNearBottom({ scrollTop: 300, scrollHeight: 1000, clientHeight: 600 })).toBe(false);
  });
});

describe("tailSignature (what makes the «Nuovi messaggi» pill appear)", () => {
  const base = [user("u1"), text("a1", "Ecco")];

  it("does not change while the rows above the end change: nothing new arrived", () => {
    const edited = [user("u1"), { ...(base[1] as Extract<Item, { kind: "text" }>) }];
    expect(tailSignature(edited)).toBe(tailSignature(base));
    // A row far from the end that is rewritten (a tool card that finishes, a rating) is not news either.
    const longer = [{ ...(base[0] as Extract<Item, { kind: "user" }>), text: "un testo più lungo" }, base[1]];
    expect(tailSignature(longer)).toBe(tailSignature(base));
  });

  it("changes when a row is added, the last reply grows or ends, or a note or a proposal shows up under the list", () => {
    const sig = tailSignature(base);
    expect(tailSignature([...base, user("u2")])).not.toBe(sig);
    expect(tailSignature([user("u1"), text("a1", "Ecco qui", true)])).not.toBe(sig);
    expect(tailSignature([user("u1"), text("a1", "Ecco", true)])).not.toBe(sig);
    expect(tailSignature(base, { notice: true })).not.toBe(sig);
    expect(tailSignature(base, { footer: true })).not.toBe(sig);
    expect(tailSignature(base, { notice: true })).not.toBe(tailSignature(base, { footer: true }));
  });

  it("follows a tool card at the end through its states, and a confirmation or question through its answer", () => {
    const tool = (status: "running" | "done", result?: string): Item => ({ kind: "tool", id: "t", name: "shell", args: "{}", status, result });
    expect(tailSignature([tool("running")])).not.toBe(tailSignature([tool("done", "ok")]));
    const q = (outcome?: "answered"): Item => ({ kind: "question", id: "q", q: { id: "q", run_id: "r", kind: "choice", title: "?", message: "", options: [], multiple: false, allow_other: false, fields: [], confirm_label: null, deny_label: null }, outcome });
    expect(tailSignature([q()])).not.toBe(tailSignature([q("answered")]));
  });

  it("is the same for an empty conversation every time", () => {
    expect(tailSignature([])).toBe(tailSignature([]));
    expect(tailSignature([])).not.toBe(tailSignature(base));
  });
});
