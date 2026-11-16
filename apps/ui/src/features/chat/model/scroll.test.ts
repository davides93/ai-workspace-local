import { describe, expect, it } from "vitest";
import type { Item } from "./items";
import { coldBefore, distanceFromBottom, isNearBottom, tailSignature } from "./scroll";

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

  it("does not change while a row above the end is rewritten: nothing new arrived", () => {
    // A row far from the end that changes (a tool card that finishes, a rating) is not news.
    const longer = [{ ...(base[0] as Extract<Item, { kind: "user" }>), text: "un testo più lungo" }, base[1]];
    expect(tailSignature(longer)).toBe(tailSignature(base));
    expect(tailSignature([...base])).toBe(tailSignature(base));
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

  it("follows the other kinds of last row too: a sub-agent that ends, a surface the server updates, a note or an error that appears", () => {
    const sub = (result?: string): Item => ({ kind: "subagent", id: "s", name: "browser", result });
    expect(tailSignature([sub()])).not.toBe(tailSignature([sub("fatto")]));
    const surface = (rev: number, n: number): Item => ({ kind: "surface", id: "u", surface: { surfaceId: "u", components: Object.fromEntries(Array.from({ length: n }, (_, i) => [`c${i}`, {} as never])), data: {}, rev } });
    expect(tailSignature([surface(1, 2)])).not.toBe(tailSignature([surface(2, 2)]));
    expect(tailSignature([surface(1, 2)])).not.toBe(tailSignature([surface(1, 3)]));
    expect(tailSignature([{ kind: "error", id: "e", message: "" }])).not.toBe(tailSignature([{ kind: "error", id: "e", message: "il runtime si è chiuso" }]));
    expect(tailSignature([{ kind: "note", id: "n", text: "a" }])).not.toBe(tailSignature([{ kind: "note", id: "n", text: "ab" }]));
  });

  it("is the same for an empty conversation every time", () => {
    expect(tailSignature([])).toBe(tailSignature([]));
    expect(tailSignature([])).not.toBe(tailSignature(base));
  });
});

describe("coldBefore (which rows are far from the end)", () => {
  it("is the number of rows before the last 40 of the conversation as it opened; a short one has none", () => {
    expect(coldBefore(0)).toBe(0);
    expect(coldBefore(40)).toBe(0);
    expect(coldBefore(41)).toBe(1);
    expect(coldBefore(360)).toBe(320);
    expect(coldBefore(100, 10)).toBe(90);
  });
});
