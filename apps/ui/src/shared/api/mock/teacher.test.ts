import { describe, expect, it } from "vitest";
import type { Memory } from "../types";
import { asNote, dueFrom, guessKind, splitFacts, teacherTurn } from "./teacher";

const mem = (id: string, text: string, status: Memory["status"] = "active"): Memory => ({
  id, text, kind: "fact", scope: "all", pinned: false, created_at: "2026-01-01T00:00:00Z", updated_at: "2026-01-01T00:00:00Z", status,
});

describe("the demo Insegnante", () => {
  it("saves one memory per distinct fact, written as a note, with the right kind", () => {
    const t = teacherTurn("Mi chiamo Giulia e preferisco che mi dai del tu.", []);
    expect(t.steps.map((s) => (s.tool === "memory_save" ? [s.args.text, s.args.kind, s.args.explicit] : null))).toEqual([
      ["Si chiama Giulia.", "about_you", true],
      ["Preferisce che mi dai del tu.", "preference", true],
    ]);
    expect(t.reply).toContain("Ricordate 2 cose");
  });
  it("pins a name and a rule of work, not a plain fact", () => {
    const steps = teacherTurn("Mi chiamo Anna. Rispondi sempre in modo breve. Ho due gatti.", []).steps;
    expect(steps.map((s) => s.tool === "memory_save" && !!s.args.pinned)).toEqual([true, true, false]);
  });
  it("gives a reminder its date", () => {
    const now = new Date("2026-06-10T10:00:00");
    const [s] = teacherTurn("Ricordami di rinnovare l'abbonamento entro venerdì.", [], now).steps;
    expect(s.tool === "memory_save" && s.args.kind).toBe("reminder");
    const due = s.tool === "memory_save" ? s.args.due_at : undefined;
    expect(new Date(due!).getDay()).toBe(5);
    expect(new Date(due!).getTime()).toBeGreaterThan(now.getTime());
  });
  it("answers «cosa sai di me?» from what it knows, and says so when it knows nothing", () => {
    const some = teacherTurn("Cosa sai di me?", [mem("a", "Si chiama Giulia."), mem("b", "Forse ama il jazz.", "proposed")]);
    expect(some.steps[0].tool).toBe("memory_search");
    expect(some.reply).toContain("Si chiama Giulia.");
    expect(some.reply).not.toContain("jazz");
    expect(teacherTurn("cosa sai di me", []).reply).toContain("Non so ancora nulla");
  });
  it("forgets the memory that matches, or says it found none", () => {
    const known = [mem("a", "Beve il caffè senza zucchero."), mem("b", "Ha due gatti.")];
    const t = teacherTurn("Dimentica il caffè", known);
    expect(t.steps).toEqual([{ tool: "memory_forget", args: { id: "a" }, summary: "Beve il caffè senza zucchero." }]);
    expect(t.reply).toContain("ripristinarlo");
    expect(teacherTurn("dimentica la zebra", known)).toEqual({ steps: [], reply: expect.stringContaining("Non trovo nulla") });
  });
  it("applies a correction to the memory it matches", () => {
    const t = teacherTurn("In realtà ho tre gatti", [mem("b", "Ha due gatti.")]);
    expect(t.steps).toEqual([{ tool: "memory_update", args: { id: "b", text: "Ha tre gatti." }, summary: "Ha tre gatti." }]);
  });
  it("asks one short question instead of saving something it did not understand", () => {
    const t = teacherTurn("ok", []);
    expect(t.steps).toEqual([]);
    expect(t.reply.endsWith("?")).toBe(true);
  });
  it("saves at most six facts from one message", () => {
    expect(splitFacts("a1. b2. c3. d4. e5. f6. g7. h8")).toHaveLength(6);
  });
  it("has small helpers that are exact", () => {
    expect(asNote("vivo a Torino")).toBe("Vive a Torino.");
    expect(asNote("la sede è a Roma")).toBe("La sede è a Roma.");
    expect(guessKind("Preferisco il caffè")).toBe("preference");
    expect(guessKind("La sede è a Roma")).toBe("fact");
    expect(dueFrom("niente date", new Date())).toBeUndefined();
    expect(new Date(dueFrom("domani", new Date("2026-06-10T10:00:00"))!).getDate()).toBe(11);
  });
});
