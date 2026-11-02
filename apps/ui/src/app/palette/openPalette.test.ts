import { describe, expect, it, vi } from "vitest";
import { onOpenPalette, openPalette } from "./openPalette";

describe("openPalette", () => {
  it("tells every subscriber, and stops after unsubscribing", () => {
    const a = vi.fn();
    const b = vi.fn();
    const offA = onOpenPalette(a);
    const offB = onOpenPalette(b);
    openPalette();
    expect([a.mock.calls.length, b.mock.calls.length]).toEqual([1, 1]);
    offA();
    openPalette();
    expect([a.mock.calls.length, b.mock.calls.length]).toEqual([1, 2]);
    offB();
    openPalette();
    expect(b.mock.calls.length).toBe(2);
  });
  it("is harmless with nobody listening", () => {
    expect(() => openPalette()).not.toThrow();
  });
  it("lets a handler unsubscribe while it runs", () => {
    const calls: string[] = [];
    const off = onOpenPalette(() => { calls.push("once"); off(); });
    const keep = onOpenPalette(() => calls.push("keep"));
    openPalette();
    openPalette();
    keep();
    expect(calls).toEqual(["once", "keep", "keep"]);
  });
});
