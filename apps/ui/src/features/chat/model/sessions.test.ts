import { describe, expect, it } from "vitest";
import type { Session } from "../../../shared/api/types";
import { dayGroup, filterSessions, groupSessions, validateTitle } from "./sessions";

// Wednesday 4 November 2026, 15:00 local time.
const now = new Date(2026, 10, 4, 15, 0, 0);
const at = (dayOffset: number, hour = 10) => new Date(2026, 10, 4 + dayOffset, hour, 0, 0).toISOString();
const session = (id: string, title: string, updated: string, agent = "operator"): Session => ({ id, title, agent_id: agent, created_at: updated, updated_at: updated });

describe("dayGroup", () => {
  it("buckets by calendar day in local time", () => {
    expect(dayGroup(at(0, 0), now)).toBe("today");
    expect(dayGroup(at(0, 23), now)).toBe("today");
    expect(dayGroup(at(-1, 23), now)).toBe("yesterday");
    expect(dayGroup(at(-2), now)).toBe("week");
    expect(dayGroup(at(-6), now)).toBe("week");
    expect(dayGroup(at(-7), now)).toBe("older");
    expect(dayGroup(at(-90), now)).toBe("older");
  });
  it("counts a future date as today and an invalid one as older", () => {
    expect(dayGroup(at(2), now)).toBe("today");
    expect(dayGroup("boh", now)).toBe("older");
  });
});

describe("groupSessions", () => {
  const list = [
    session("a", "Vecchia", at(-30)),
    session("b", "Oggi 1", at(0, 9)),
    session("c", "Ieri", at(-1)),
    session("d", "Oggi 2", at(0, 14)),
    session("e", "Giorni fa", at(-3)),
  ];
  it("orders the groups Oggi, Ieri, Questa settimana, Prima and each group newest first", () => {
    const groups = groupSessions(list, now);
    expect(groups.map((g) => g.label)).toEqual(["Oggi", "Ieri", "Questa settimana", "Prima"]);
    expect(groups[0].sessions.map((s) => s.id)).toEqual(["d", "b"]);
  });
  it("leaves out empty groups and does not touch the input", () => {
    const only = [list[0]];
    expect(groupSessions(only, now).map((g) => g.id)).toEqual(["older"]);
    expect(groupSessions([], now)).toEqual([]);
    expect(list[0].id).toBe("a");
  });
});

describe("filterSessions", () => {
  const list = [session("a", "Riordino PDF in Downloads", at(0)), session("b", "Prenotazione sala", at(0), "browser")];
  const name = (id: string) => ({ operator: "Operatore", browser: "Navigatore" })[id];
  it("matches title words regardless of case and accents", () => {
    expect(filterSessions(list, "pdf downloads", name).map((s) => s.id)).toEqual(["a"]);
    expect(filterSessions(list, "SALA", name).map((s) => s.id)).toEqual(["b"]);
  });
  it("also matches the agent name", () => {
    expect(filterSessions(list, "navigatore", name).map((s) => s.id)).toEqual(["b"]);
  });
  it("returns everything for an empty query and nothing for a miss", () => {
    expect(filterSessions(list, "  ", name)).toBe(list);
    expect(filterSessions(list, "zzz", name)).toEqual([]);
  });
});

describe("validateTitle", () => {
  it("collapses spaces and rejects empty or oversized titles", () => {
    expect(validateTitle("  Nuovo   titolo \n")).toEqual({ ok: true, title: "Nuovo titolo" });
    expect(validateTitle("   ")).toMatchObject({ ok: false });
    expect(validateTitle("x".repeat(200))).toMatchObject({ ok: true });
    expect(validateTitle("x".repeat(201))).toMatchObject({ ok: false });
  });
});
