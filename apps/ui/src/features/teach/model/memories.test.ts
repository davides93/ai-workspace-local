import { describe, expect, it } from "vitest";
import type { Memory, Session } from "../../../shared/api/types";
import {
  activated, byImportance, dueLabel, EXAMPLES, exportFileName, exportJson, forgotten, KIND_LABEL, kindLabel, patched, splitMemories, teachSession, without,
} from "./memories";

const mem = (id: string, over: Partial<Memory> = {}): Memory => ({
  id, text: `testo ${id}`, kind: "fact", scope: "all", pinned: false, created_at: "2026-01-01T00:00:00Z", updated_at: "2026-01-01T00:00:00Z", status: "active", ...over,
});

describe("group names", () => {
  it("uses the Italian names of the brief", () => {
    expect(Object.values(KIND_LABEL)).toEqual(["Su di te", "Preferenze", "Come lavorare", "Fatti", "Promemoria"]);
  });
  it("shows an unknown kind as a fact", () => {
    expect(kindLabel("qualcosa")).toBe("Fatti");
    expect(kindLabel("reminder")).toBe("Promemoria");
  });
  it("has three example sentences", () => {
    expect(EXAMPLES).toHaveLength(3);
    expect(new Set(EXAMPLES).size).toBe(3);
  });
});

describe("splitMemories", () => {
  const list = [
    mem("a", { kind: "preference", updated_at: "2026-02-01T00:00:00Z" }),
    mem("b", { kind: "about_you", pinned: true }),
    mem("c", { kind: "preference", pinned: true, updated_at: "2026-01-01T00:00:00Z" }),
    mem("d", { kind: "reminder", status: "proposed" }),
    mem("e", { kind: "fact", status: "forgotten" }),
    mem("f", { kind: "preference", updated_at: "2026-03-01T00:00:00Z" }),
  ];
  it("groups the active ones by kind in the fixed order and drops empty groups", () => {
    const s = splitMemories(list);
    expect(s.groups.map((g) => g.label)).toEqual(["Su di te", "Preferenze"]);
    expect(s.known).toBe(4);
  });
  it("puts pinned first, then the most recently changed", () => {
    expect(splitMemories(list).groups[1].items.map((m) => m.id)).toEqual(["c", "f", "a"]);
  });
  it("keeps the proposals apart and hides the forgotten ones", () => {
    const s = splitMemories(list);
    expect(s.proposed.map((m) => m.id)).toEqual(["d"]);
    expect(s.groups.flatMap((g) => g.items).some((m) => m.id === "e" || m.id === "d")).toBe(false);
  });
  it("is empty for no data", () => {
    expect(splitMemories(undefined)).toEqual({ proposed: [], groups: [], known: 0 });
  });
  it("does not lose a kind from a newer engine", () => {
    const s = splitMemories([mem("x", { kind: "nuovo" as never })]);
    expect(s.groups.map((g) => g.kind)).toEqual(["fact"]);
  });
  it("orders by pinned, date, id", () => {
    expect([mem("b"), mem("a")].sort(byImportance).map((m) => m.id)).toEqual(["a", "b"]);
  });
});

describe("optimistic updates", () => {
  const list = [mem("a", { due_at: "2026-12-01T00:00:00Z" }), mem("b")];
  it("applies a patch like the engine: text on one line, an empty date clears it", () => {
    const r = patched(list, "a", { text: "  due   righe ", pinned: true, kind: "reminder", due_at: "" });
    expect(r[0]).toMatchObject({ text: "due righe", pinned: true, kind: "reminder" });
    expect(r[0].due_at).toBeUndefined();
    expect(r[1]).toBe(list[1]);
    expect(list[0].due_at).toBeDefined();
  });
  it("forgets by removing, or by marking in the list that shows everything", () => {
    expect(forgotten(list, "a").map((m) => m.id)).toEqual(["b"]);
    expect(forgotten(list, "a", true)[0].status).toBe("forgotten");
  });
  it("activates a restored or approved memory at the top", () => {
    const r = activated([mem("a", { status: "proposed" }), mem("b")], mem("a", { status: "proposed" }));
    expect(r.map((m) => `${m.id}:${m.status}`)).toEqual(["a:active", "b:active"]);
  });
  it("removes by id", () => {
    expect(without(list, "b").map((m) => m.id)).toEqual(["a"]);
  });
});

describe("dueLabel", () => {
  // Local times: «oggi» and «domani» are calendar days of the person's own time zone.
  const now = new Date(2026, 5, 10, 10, 0).getTime();
  const at = (d: number, h: number) => new Date(2026, 5, d, h, 0).toISOString();
  it("tells how long is left", () => {
    expect(dueLabel(undefined, now)).toBeUndefined();
    expect(dueLabel("boh", now)).toBeUndefined();
    expect(dueLabel(at(9, 10), now)).toBe("scaduto");
    expect(dueLabel(at(10, 18), now)).toBe("scade oggi");
    expect(dueLabel(at(11, 9), now)).toBe("scade domani");
    expect(dueLabel(at(15, 10), now)).toBe("scade tra 5 giorni");
  });
});

describe("export", () => {
  it("names the file after the date", () => {
    expect(exportFileName(new Date("2026-11-11T08:00:00Z"))).toBe("ricordi-2026-11-11.json");
  });
  it("exports what is in use and the proposals, not the forgotten ones", () => {
    const out = JSON.parse(exportJson([mem("a", { pinned: true, kind: "about_you" }), mem("b", { status: "proposed" }), mem("c", { status: "forgotten" })]));
    expect(out.versione).toBe(1);
    expect(out.ricordi.map((r: { id: string }) => r.id)).toEqual(["a", "b"]);
    expect(out.ricordi[0]).toMatchObject({ testo: "testo a", tipo: "Su di te", sempre_presente: true, stato: "attivo" });
    expect(out.ricordi[1].stato).toBe("da confermare");
  });
});

describe("teachSession", () => {
  const s = (id: string, agent: string, updated: string): Session => ({ id, title: id, agent_id: agent, created_at: updated, updated_at: updated });
  it("is the latest session of the teacher, never another agent's", () => {
    expect(teachSession([s("x", "operator", "2026-05-01"), s("t1", "teacher", "2026-02-01"), s("t2", "teacher", "2026-03-01")])?.id).toBe("t2");
    expect(teachSession([s("x", "operator", "2026-05-01")])).toBeUndefined();
    expect(teachSession(undefined)).toBeUndefined();
  });
});
