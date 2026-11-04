import { describe, expect, it } from "vitest";
import { describeCron, nextCronRun, parseCron } from "./cron";

describe("parseCron", () => {
  it("expands lists, ranges and steps", () => {
    const r = parseCron("*/15 8-10 1,15 * 1-5");
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect([...r.fields.minute]).toEqual([0, 15, 30, 45]);
    expect([...r.fields.hour]).toEqual([8, 9, 10]);
    expect([...r.fields.day]).toEqual([1, 15]);
    expect(r.fields.month.size).toBe(12);
    expect([...r.fields.weekday]).toEqual([1, 2, 3, 4, 5]);
  });

  it("reads 0 and 7 as Sunday and accepts names", () => {
    const r = parseCron("0 0 * jan,mar sun,7");
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect([...r.fields.weekday]).toEqual([0]);
    expect([...r.fields.month]).toEqual([1, 3]);
    const mon = parseCron("0 0 * * Mon-Fri");
    expect(mon.ok && [...mon.fields.weekday]).toEqual([1, 2, 3, 4, 5]);
  });

  it("starts a stepped single value at that value (5/20 → 5, 25, 45)", () => {
    const r = parseCron("5/20 * * * *");
    expect(r.ok && [...r.fields.minute]).toEqual([5, 25, 45]);
  });

  it("needs exactly five fields", () => {
    for (const bad of ["", "* * * *", "* * * * * *", "0 0 8 * * 1"]) {
      const r = parseCron(bad);
      expect(r.ok, bad).toBe(false);
      if (!r.ok) expect(r.error).toMatch(/5 campi/);
    }
  });

  it("says which field is wrong", () => {
    const cases: [string, RegExp][] = [
      ["60 * * * *", /minuti/],
      ["* 24 * * *", /ore/],
      ["* * 0 * *", /giorno del mese/],
      ["* * * 13 *", /mese/],
      ["* * * * 8", /giorno della settimana/],
      ["* * * * x", /non è un valore valido/],
      ["5-1 * * * *", /al contrario/],
      ["*/0 * * * *", /passo/],
      ["1,,2 * * * *", /vuoto/],
    ];
    for (const [expr, re] of cases) {
      const r = parseCron(expr);
      expect(r.ok, expr).toBe(false);
      if (!r.ok) expect(r.error, expr).toMatch(re);
    }
  });
});

const at = (y: number, mo: number, d: number, h: number, mi: number) => new Date(y, mo - 1, d, h, mi);
const parts = (d: Date | null) => d && [d.getFullYear(), d.getMonth() + 1, d.getDate(), d.getHours(), d.getMinutes()];

describe("nextCronRun (local time)", () => {
  it("finds the next weekday morning, like the backend test (Saturday → Monday)", () => {
    // 2026-10-31 is a Saturday: next run of `0 8 * * 1-5` is Monday 2 November at 08:00.
    expect(parts(nextCronRun("0 8 * * 1-5", at(2026, 10, 31, 9, 0)))).toEqual([2026, 11, 2, 8, 0]);
  });

  it("never returns the instant it was asked from", () => {
    expect(parts(nextCronRun("30 9 * * *", at(2026, 11, 2, 9, 30)))).toEqual([2026, 11, 3, 9, 30]);
    expect(parts(nextCronRun("30 9 * * *", at(2026, 11, 2, 9, 29)))).toEqual([2026, 11, 2, 9, 30]);
  });

  it("handles steps, months and rolling over the year", () => {
    expect(parts(nextCronRun("*/20 * * * *", at(2026, 11, 2, 10, 5)))).toEqual([2026, 11, 2, 10, 20]);
    expect(parts(nextCronRun("0 0 1 1 *", at(2026, 11, 2, 10, 5)))).toEqual([2027, 1, 1, 0, 0]);
  });

  it("needs the day of the month AND the weekday to match, as the cron crate does", () => {
    // The first of the month that is also a Monday: 2026-06-01.
    expect(parts(nextCronRun("0 0 1 * 1", at(2026, 3, 1, 0, 0)))).toEqual([2026, 6, 1, 0, 0]);
  });

  it("gives up on an expression that never fires", () => {
    expect(nextCronRun("0 0 31 2 *", at(2026, 1, 1, 0, 0))).toBeNull();
    expect(nextCronRun("not cron", at(2026, 1, 1, 0, 0))).toBeNull();
  });
});

describe("describeCron", () => {
  it("reads the common shapes in Italian", () => {
    expect(describeCron("0 8 * * 1-5")).toBe("Giorni feriali alle 08:00");
    expect(describeCron("30 7 * * *")).toBe("Ogni giorno alle 07:30");
    expect(describeCron("0 9 * * 1")).toBe("Ogni lunedì alle 09:00");
    expect(describeCron("0 10 * * 0,6")).toBe("Sabato e domenica alle 10:00");
    expect(describeCron("0 9 1 * *")).toBe("Il 1 di ogni mese alle 09:00");
    expect(describeCron("*/10 * * * *")).toBe("Ogni 10 minuti");
    expect(describeCron("0 */4 * * *")).toBe("Ogni 4 ore");
  });

  it("leaves unusual or invalid expressions to the raw text", () => {
    expect(describeCron("0 8 * 6 1-5")).toBeNull();
    expect(describeCron("5 8,12 * * *")).toBeNull();
    expect(describeCron("nope")).toBeNull();
  });
});
