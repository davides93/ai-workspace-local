import { describe, expect, it } from "vitest";
import { chatPathForRun, forgetSessionRuns, rememberRun, sessionForRun } from "./runIndex";

describe("run → session index", () => {
  it("knows the session of a run it has seen and links to its chat", () => {
    rememberRun("run_a", "ses 1");
    expect(sessionForRun("run_a")).toBe("ses 1");
    expect(chatPathForRun("run_a")).toBe("/chat/ses%201");
  });
  it("does not know a run it has not seen, or an empty one", () => {
    expect(sessionForRun("run_zz")).toBeUndefined();
    expect(chatPathForRun(undefined)).toBeUndefined();
    expect(chatPathForRun(null)).toBeUndefined();
    rememberRun("", "ses");
    rememberRun("run_x", "");
    expect(sessionForRun("run_x")).toBeUndefined();
  });
  it("forgets the runs of a deleted session only", () => {
    rememberRun("run_b", "s2");
    rememberRun("run_c", "s2");
    rememberRun("run_d", "s3");
    forgetSessionRuns("s2");
    expect(sessionForRun("run_b")).toBeUndefined();
    expect(sessionForRun("run_c")).toBeUndefined();
    expect(sessionForRun("run_d")).toBe("s3");
  });
  it("keeps a bounded memory: the oldest runs go first", () => {
    for (let i = 0; i < 450; i++) rememberRun(`bulk_${i}`, "s_bulk");
    expect(sessionForRun("bulk_0")).toBeUndefined();
    expect(sessionForRun("bulk_449")).toBe("s_bulk");
  });
});
