import { afterEach, describe, expect, it } from "vitest";
import { onComposerSeed, seedComposer, takeComposerSeed, withSeed } from "./composerSeed";

afterEach(() => void takeComposerSeed());

describe("composer seed", () => {
  it("hands the text over once", () => {
    seedComposer("Scarica un modello");
    expect(takeComposerSeed()).toBe("Scarica un modello");
    expect(takeComposerSeed()).toBeNull();
  });
  it("keeps only the last request", () => {
    seedComposer("uno");
    seedComposer("due");
    expect(takeComposerSeed()).toBe("due");
  });
  it("tells the chat when there is a request, until it stops listening", () => {
    let calls = 0;
    const off = onComposerSeed(() => calls++);
    seedComposer("a");
    seedComposer("b");
    off();
    seedComposer("c");
    expect(calls).toBe(2);
  });
  it("adds the text below what is already typed, or alone when the box is empty", () => {
    expect(withSeed("", "Scarica un modello")).toBe("Scarica un modello");
    expect(withSeed("  \n", "Scarica un modello")).toBe("Scarica un modello");
    expect(withSeed("Ciao", "Scarica un modello")).toBe("Ciao\nScarica un modello");
    expect(withSeed("Ciao  \n", "Scarica un modello")).toBe("Ciao\nScarica un modello");
  });
});
