import { describe, expect, it } from "vitest";
import type { ReleaseInfo } from "../../../shared/api/types";
import { downloadPercent, updateStep } from "./update";

const release = (asset: boolean): ReleaseInfo => ({
  tag: "v0.5.0", version: "0.5.0", channel: "stable", notes: "", page: "https://github.com/o/r/releases/tag/v0.5.0",
  asset: asset ? { name: "a.dmg", size: 10, url: "https://github.com/o/r/a.dmg" } : null,
});

describe("updateStep", () => {
  it("walks from nothing to download, install and the manual case", () => {
    expect(updateStep({ state: "idle", available: null, file: null })).toBe("none");
    expect(updateStep({ state: "idle", available: release(true), file: null })).toBe("download");
    expect(updateStep({ state: "downloading", available: release(true), file: null })).toBe("downloading");
    expect(updateStep({ state: "ready", available: release(true), file: "/u/a.dmg" })).toBe("install");
    expect(updateStep({ state: "idle", available: release(false), file: null })).toBe("manual");
  });
});

describe("downloadPercent", () => {
  it("is a percentage, 0 while the size is unknown", () => {
    expect(downloadPercent({ received: 0, total: 0 })).toBe(0);
    expect(downloadPercent({ received: 25, total: 100 })).toBe(25);
    expect(downloadPercent({ received: 120, total: 100 })).toBe(100);
  });
});
