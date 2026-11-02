import { describe, expect, it } from "vitest";
import { formatBytes, formatCompact, formatContext, formatDuration, formatMb, formatNumber, formatParams, formatPercent, formatSpeed, relTime } from "./format";

const GB = 1024 ** 3;

describe("formatNumber / formatPercent", () => {
  it("uses the decimal comma of it-IT", () => {
    expect(formatNumber(1.5)).toBe("1,5");
    expect(formatNumber(1234.56, 2)).toBe("1234,56");
    expect(formatPercent(0.75)).toBe("75%");
  });
  it("compacts large counts", () => {
    expect(formatCompact(1500)).toMatch(/^1,5\s?K$/);
    expect(formatCompact(2_300_000)).toMatch(/Mln/);
  });
});

describe("formatBytes", () => {
  it("scales units with a decimal comma", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(212 * 1024)).toBe("212 KB");
    expect(formatBytes(1.5 * GB)).toBe("1,5 GB");
    expect(formatBytes(21.2 * GB)).toBe("21,2 GB");
    expect(formatBytes(290 * GB)).toBe("290 GB");
  });
  it("renders empty values as a dash", () => {
    expect(formatBytes(0)).toBe("—");
    expect(formatBytes(undefined)).toBe("—");
    expect(formatBytes(Number.NaN)).toBe("—");
  });
  it("formats speed", () => {
    expect(formatSpeed(12.4 * 1024 ** 2)).toBe("12,4 MB/s");
    expect(formatSpeed(0)).toBe("—");
  });
});

describe("formatMb", () => {
  it("switches to GB from 1024 MB", () => {
    expect(formatMb(512)).toBe("512 MB");
    expect(formatMb(1536)).toBe("1,5 GB");
    expect(formatMb(24564)).toBe("24 GB");
    expect(formatMb(65536)).toBe("64 GB");
    expect(formatMb(undefined)).toBe("—");
  });
});

describe("model facts", () => {
  it("formats parameter counts and contexts", () => {
    expect(formatParams(35)).toBe("35B");
    expect(formatParams(0.6)).toBe("0,6B");
    expect(formatParams(undefined)).toBe("—");
    expect(formatContext(32768)).toBe("32K");
    expect(formatContext(undefined)).toBe("—");
  });
});

describe("formatDuration", () => {
  it("picks the unit", () => {
    expect(formatDuration(35)).toBe("35 s");
    expect(formatDuration(240)).toBe("4 min");
    expect(formatDuration(4920)).toBe("1 h 22 min");
    expect(formatDuration(3 * 86400)).toBe("3 gg");
  });
});

describe("relTime", () => {
  const now = Date.parse("2026-11-02T12:00:00Z");
  const ago = (s: number) => new Date(now - s * 1000).toISOString();
  it("is compact and Italian", () => {
    expect(relTime(ago(10), now)).toBe("ora");
    expect(relTime(ago(5 * 60), now)).toBe("5 min fa");
    expect(relTime(ago(3 * 3600), now)).toBe("3 h fa");
    expect(relTime(ago(2 * 86400), now)).toBe("2 gg fa");
  });
  it("tolerates invalid input", () => {
    expect(relTime("not a date", now)).toBe("—");
  });
});
