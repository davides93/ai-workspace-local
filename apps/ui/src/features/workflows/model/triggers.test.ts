import { describe, expect, it } from "vitest";
import { intervalLabel, joinInterval, runSeconds, splitInterval, summarizeTrigger, triggerLabel } from "./triggers";

const now = new Date(2026, 10, 2, 10, 0, 0);

describe("intervals", () => {
  it("splits seconds into the largest even unit", () => {
    expect(splitInterval(21600)).toEqual({ amount: 6, unit: "h" });
    expect(splitInterval(5400)).toEqual({ amount: 90, unit: "min" });
    expect(splitInterval(45)).toEqual({ amount: 45, unit: "s" });
    expect(splitInterval(0)).toEqual({ amount: 0, unit: "s" });
  });

  it("joins them back", () => {
    expect(joinInterval(6, "h")).toBe(21600);
    expect(joinInterval(1.5, "min")).toBe(90);
    expect(joinInterval(30, "s")).toBe(30);
  });

  it("labels them", () => {
    expect(intervalLabel(21600)).toBe("Ogni 6 h");
    expect(intervalLabel(3600)).toBe("Ogni 1 ora");
    expect(intervalLabel(900)).toBe("Ogni 15 min");
    expect(intervalLabel(45)).toBe("Ogni 45 s");
  });
});

describe("trigger labels", () => {
  it("covers every kind", () => {
    expect(triggerLabel({ kind: "manual" })).toBe("Manuale");
    expect(triggerLabel({ kind: "webhook" })).toBe("Webhook");
    expect(triggerLabel({ kind: "file", path: "~/a.txt" })).toBe("Quando cambia ~/a.txt");
    expect(triggerLabel({ kind: "cron", expr: "0 8 * * 1-5" })).toBe("Giorni feriali alle 08:00");
    expect(triggerLabel({ kind: "cron", expr: "5 8,12 * * *" })).toBe("Cron 5 8,12 * * *");
  });
});

describe("summarizeTrigger", () => {
  it("computes the next start of an enabled cron workflow", () => {
    const s = summarizeTrigger({ kind: "cron", expr: "0 8 * * *" }, { enabled: true, now });
    expect(s.next && [s.next.getDate(), s.next.getHours()]).toEqual([3, 8]);
  });

  it("has no next start when disabled, and says so for scheduled kinds only", () => {
    expect(summarizeTrigger({ kind: "cron", expr: "0 8 * * *" }, { enabled: false, now })).toEqual({ label: "Ogni giorno alle 08:00", note: "Disattivato: non parte da solo" });
    expect(summarizeTrigger({ kind: "manual" }, { enabled: false, now }).note).toBeUndefined();
    expect(summarizeTrigger({ kind: "webhook" }, { enabled: false, now }).note).toBeUndefined();
  });

  it("estimates an interval from the last run and never from the past", () => {
    const last = { started_at: new Date(2026, 10, 2, 9, 0, 0).toISOString() };
    const s = summarizeTrigger({ kind: "interval", seconds: 3600 }, { enabled: true, lastRun: last, now });
    expect(s.next?.getHours()).toBe(11);
    const old = { started_at: new Date(2026, 10, 1, 9, 30, 0).toISOString() };
    const t = summarizeTrigger({ kind: "interval", seconds: 3600 }, { enabled: true, lastRun: old, now });
    expect(t.next && t.next.getTime() > now.getTime()).toBe(true);
    expect(t.next?.getMinutes()).toBe(30);
  });

  it("does not guess without a last run", () => {
    const s = summarizeTrigger({ kind: "interval", seconds: 3600 }, { enabled: true, now });
    expect(s.next).toBeUndefined();
    expect(s.note).toMatch(/dopo l'avvio/);
  });

  it("applies the 30 second floor of the scheduler", () => {
    const last = { started_at: new Date(2026, 10, 2, 9, 59, 50).toISOString() };
    const s = summarizeTrigger({ kind: "interval", seconds: 5 }, { enabled: true, lastRun: last, now });
    expect(s.next && s.next.getTime() - now.getTime()).toBe(20_000);
  });
});

describe("runSeconds", () => {
  it("measures finished and running runs", () => {
    expect(runSeconds("2026-11-02T10:00:00Z", "2026-11-02T10:01:30Z")).toBe(90);
    expect(runSeconds("2026-11-02T10:00:00Z", undefined, Date.parse("2026-11-02T10:00:07Z"))).toBe(7);
    expect(runSeconds("garbage", undefined)).toBeNull();
  });
});
