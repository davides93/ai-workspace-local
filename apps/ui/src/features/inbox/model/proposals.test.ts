import { describe, expect, it } from "vitest";
import type { ProactiveStatus, Proposal } from "../../../shared/api/types";
import { badgeCount, decided, editedInstruction, latestBriefing, statusLine, waiting } from "./proposals";

const p = (over: Partial<Proposal>): Proposal => ({
  id: "p", kind: "proposal", title: "T", noticed: "N", instruction: "Fai X", source: "s", agent_id: "operator", status: "open",
  created_at: "2026-06-10T08:00:00Z", expires_at: "2026-06-17T08:00:00Z", ...over,
});

describe("proposals", () => {
  const list = [
    p({ id: "a" }),
    p({ id: "b", status: "done", decided_at: "2026-06-10T09:00:00Z" }),
    p({ id: "c", status: "dismissed", decided_at: "2026-06-10T10:00:00Z" }),
    p({ id: "d", kind: "briefing", created_at: "2026-06-10T07:00:00Z" }),
    p({ id: "e", kind: "briefing", created_at: "2026-06-11T07:00:00Z" }),
    p({ id: "f", kind: "briefing", status: "done" }),
  ];

  it("counts only open proposals on the badge, never the briefing", () => {
    expect(waiting(list).map((x) => x.id)).toEqual(["a"]);
    expect(badgeCount(list)).toBe(1);
    expect(badgeCount(undefined)).toBe(0);
  });

  it("shows the newest open briefing and skips the 'nothing to report' ones", () => {
    expect(latestBriefing(list)?.id).toBe("e");
    expect(latestBriefing([p({ kind: "briefing", status: "done" })])).toBeUndefined();
  });

  it("lists what was decided, latest first, without briefings", () => {
    expect(decided(list).map((x) => x.id)).toEqual(["c", "b"]);
  });

  it("sends an edited instruction only when it differs", () => {
    expect(editedInstruction(p({}), " Fai X ")).toBeUndefined();
    expect(editedInstruction(p({}), "Fai Y")).toBe("Fai Y");
    expect(editedInstruction(p({}), "  ")).toBeUndefined();
  });

  it("says in plain words why the assistant is quiet", () => {
    const base: ProactiveStatus = { level: "propose", budget_used_minutes: 0, budget_minutes: 15, open_proposals: 0, stopped_by_kill_switch: false };
    const now = Date.parse("2026-06-10T08:00:00Z");
    expect(statusLine(undefined)).toBe("");
    expect(statusLine({ ...base, level: "off" })).toMatch(/spento/);
    expect(statusLine({ ...base, stopped_by_kill_switch: true })).toMatch(/emergenza/);
    expect(statusLine({ ...base, skipped_reason: "Orario di silenzio." })).toBe("Orario di silenzio.");
    expect(statusLine({ ...base, next_wake: "2026-06-10T08:30:00Z" }, now)).toBe("Riguarda tra circa 30 minuti.");
  });
});
