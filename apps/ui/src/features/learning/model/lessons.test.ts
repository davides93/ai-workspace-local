import { describe, expect, it } from "vitest";
import { MAX_LESSON_CONTENT_CHARS, MAX_LESSON_TITLE_CHARS, type Lesson } from "../../../shared/api/types";
import { approvedMessage, checkLessonEdit, editOf, memoryPendingFor, pendingFor, rememberText, withDecided } from "./lessons";

const lesson = (id: string, session_id: string, status: Lesson["status"] = "pending"): Lesson => ({
  id, session_id, agent_id: "operator", kind: "memory", title: "Titolo", content: "Contenuto", reason: "perché", status, created_at: "t",
});

describe("checkLessonEdit", () => {
  it("trims and accepts a normal edit", () => {
    expect(checkLessonEdit("  Titolo ", " testo\n")).toEqual({ ok: true, title: "Titolo", content: "testo" });
  });
  it("refuses empty or too long text, like the engine", () => {
    expect(checkLessonEdit(" ", "x")).toMatchObject({ ok: false });
    expect(checkLessonEdit("t", " \n")).toMatchObject({ ok: false });
    expect(checkLessonEdit("t".repeat(MAX_LESSON_TITLE_CHARS + 1), "x")).toMatchObject({ ok: false, error: expect.stringContaining(String(MAX_LESSON_TITLE_CHARS)) });
    expect(checkLessonEdit("t", "x".repeat(MAX_LESSON_CONTENT_CHARS + 1))).toMatchObject({ ok: false });
    expect(checkLessonEdit("t".repeat(MAX_LESSON_TITLE_CHARS), "x".repeat(MAX_LESSON_CONTENT_CHARS)).ok).toBe(true);
  });
});

describe("editOf", () => {
  it("sends only what changed", () => {
    const l = lesson("a", "s");
    expect(editOf(l, "Titolo", "Contenuto")).toEqual({});
    expect(editOf(l, " Titolo ", "Contenuto\n")).toEqual({});
    expect(editOf(l, "Nuovo", "Contenuto")).toEqual({ title: "Nuovo" });
    expect(editOf(l, "Titolo", "Altro")).toEqual({ content: "Altro" });
  });
});

describe("lists", () => {
  it("finds the lesson waiting for a session", () => {
    const list = [lesson("a", "s1", "approved"), lesson("b", "s1"), lesson("c", "s2")];
    expect(pendingFor(list, "s1")?.id).toBe("b");
    expect(pendingFor(list, "s3")).toBeUndefined();
    expect(pendingFor(list, null)).toBeUndefined();
    expect(pendingFor(undefined, "s1")).toBeUndefined();
  });
  it("moves a decided lesson out of the pending list and into the full one", () => {
    const list = [lesson("a", "s"), lesson("b", "s")];
    const done = lesson("a", "s", "approved");
    expect(withDecided(list, done, true).map((l) => l.id)).toEqual(["b"]);
    expect(withDecided(list, done, false).map((l) => [l.id, l.status])).toEqual([["a", "approved"], ["b", "pending"]]);
    expect(withDecided(undefined, lesson("n", "s"), true).map((l) => l.id)).toEqual(["n"]);
  });
  it("says where the lesson went", () => {
    expect(approvedMessage({ kind: "skill", title: "X" })).toContain("skill");
    expect(approvedMessage({ kind: "memory", title: "X" })).toContain("AGENTS.md");
  });
});

describe("the Vuoi che lo ricordi? card", () => {
  const skill: Lesson = { ...lesson("sk", "s"), kind: "skill" };
  it("is for the pending memory lesson of the session; a skill stays in the Panoramica", () => {
    expect(memoryPendingFor([skill, lesson("m", "s")], "s")?.id).toBe("m");
    expect(memoryPendingFor([skill], "s")).toBeUndefined();
    expect(memoryPendingFor([lesson("m", "s", "approved")], "s")).toBeUndefined();
    expect(memoryPendingFor([lesson("m", "other")], "s")).toBeUndefined();
    expect(memoryPendingFor([lesson("m", "s")], null)).toBeUndefined();
  });
  it("shows the content on one line and cuts a long one", () => {
    expect(rememberText({ title: "T", content: "Prima  chiedi\n il formato." })).toBe("Prima chiedi il formato.");
    expect(rememberText({ title: "Titolo", content: "  " })).toBe("Titolo");
    const long = rememberText({ title: "T", content: "x".repeat(400) });
    expect(long).toHaveLength(221);
    expect(long.endsWith("…")).toBe(true);
  });
});
