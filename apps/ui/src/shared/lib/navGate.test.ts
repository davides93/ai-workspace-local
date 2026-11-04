import { describe, expect, it } from "vitest";
import { createNavGate } from "./navGate";

function setup(start = "#/agents/coder") {
  const loc = { hash: start };
  const gate = createNavGate(() => loc.hash);
  return { loc, gate };
}

describe("navigation gate", () => {
  it("lets a navigation through when nobody objects", () => {
    const { loc, gate } = setup();
    loc.hash = "#/models";
    expect(gate.check()).toBe(true);
    // The new hash is now the rendered one: asking again changes nothing.
    expect(gate.check()).toBe(true);
  });

  it("lets the router re-render the hash it already shows", () => {
    const { gate } = setup();
    expect(gate.check()).toBe(true);
  });

  it("asks the blockers with the target and the rendered hash, and honours a veto", () => {
    const { loc, gate } = setup();
    const seen: [string, string][] = [];
    gate.block((to, from) => {
      seen.push([to, from]);
      loc.hash = from; // the blocker puts the old hash back
      return true;
    });
    loc.hash = "#/models";
    expect(gate.check()).toBe(false);
    expect(seen).toEqual([["#/models", "#/agents/coder"]]);
    expect(loc.hash).toBe("#/agents/coder");
    // After the veto the router still shows the first hash: nothing to block now.
    expect(gate.check()).toBe(true);
  });

  it("keeps asking for the next attempt, from the hash that is really rendered", () => {
    const { loc, gate } = setup();
    let allow = false;
    gate.block((_to, from) => {
      if (allow) return false;
      loc.hash = from;
      return true;
    });
    loc.hash = "#/a";
    expect(gate.check()).toBe(false);
    loc.hash = "#/b";
    expect(gate.check()).toBe(false);
    allow = true;
    loc.hash = "#/b";
    expect(gate.check()).toBe(true);
    loc.hash = "#/c";
    expect(gate.check()).toBe(true);
  });

  it("stops asking a blocker that was removed", () => {
    const { loc, gate } = setup();
    const off = gate.block(() => true);
    off();
    loc.hash = "#/models";
    expect(gate.check()).toBe(true);
  });
});
