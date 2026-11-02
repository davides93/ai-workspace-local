import { describe, expect, it } from "vitest";
import { CONTAINMENT, KNOWN_LIMITS, containmentFor } from "./sandbox";

describe("sandbox facts", () => {
  it("covers every platform the app runs on, once", () => {
    expect(CONTAINMENT.map((c) => c.os).sort()).toEqual(["android", "ios", "linux", "macos", "windows"]);
  });

  it("finds the row of the current device", () => {
    expect(containmentFor("linux")?.mechanism).toMatch(/bubblewrap/);
    expect(containmentFor("windows")?.network).toMatch(/Non isolata/);
    expect(containmentFor("web")).toBeUndefined();
    expect(containmentFor(undefined)).toBeUndefined();
  });

  it("only offers a mechanism where the shell can run", () => {
    for (const c of CONTAINMENT) if (!c.shell) expect(c.mechanism).toBeNull();
  });

  it("keeps the network limitation first", () => {
    expect(KNOWN_LIMITS[0].title).toMatch(/rete/i);
  });
});
