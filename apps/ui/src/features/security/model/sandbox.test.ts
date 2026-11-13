import { describe, expect, it } from "vitest";
import { CONTAINMENT, KNOWN_LIMITS, containmentFor } from "./sandbox";

describe("sandbox facts", () => {
  it("covers every platform the app runs on, once", () => {
    expect(CONTAINMENT.map((c) => c.os).sort()).toEqual(["android", "ios", "linux", "macos", "windows"]);
  });

  it("finds the row of the current device", () => {
    expect(containmentFor("linux")?.mechanism).toMatch(/bubblewrap/);
    expect(containmentFor("windows")?.network).toMatch(/Non isolata/);
    expect(containmentFor("windows")?.reads).toMatch(/^No/);
    expect(containmentFor("windows")?.mcp).toMatch(/non si possono isolare/);
    expect(containmentFor("linux")?.network).toMatch(/Chiusa di default/);
    expect(containmentFor("web")).toBeUndefined();
    expect(containmentFor(undefined)).toBeUndefined();
  });

  it("only offers a mechanism where the shell can run", () => {
    for (const c of CONTAINMENT) if (!c.shell) expect(c.mechanism).toBeNull();
  });

  it("keeps the network limitation first and no longer claims the network is open by default", () => {
    expect(KNOWN_LIMITS[0].title).toMatch(/rete/i);
    expect(KNOWN_LIMITS.map((l) => l.title).join(" ")).not.toMatch(/aperta di default/i);
  });
});
