import { describe, expect, it } from "vitest";
import type { PolicyRule } from "../../../shared/api/types";
import { globMatches, globProblem } from "./glob";
import { checkRules, evaluate, moveItem, normalizeRules, ruleLabel, ruleMatches, runProbe } from "./policy";

describe("glob (globset dialect)", () => {
  it("lets * and ? cross path separators", () => {
    expect(globMatches("*.rs", "src/lib/main.rs")).toBe(true);
    expect(globMatches("src/*", "src/a/b/c")).toBe(true);
    expect(globMatches("a?c", "a/c")).toBe(true);
    expect(globMatches("a?c", "ac")).toBe(false);
  });

  it("anchors the whole value", () => {
    expect(globMatches("shell", "shell")).toBe(true);
    expect(globMatches("shell", "shell2")).toBe(false);
    expect(globMatches("hell", "shell")).toBe(false);
  });

  it("reads ** as a path component", () => {
    expect(globMatches("**/foo", "foo")).toBe(true);
    expect(globMatches("**/foo", "a/b/foo")).toBe(true);
    expect(globMatches("**/foo", "a/b/xfoo")).toBe(false);
    expect(globMatches("foo/**", "foo/bar/baz")).toBe(true);
    expect(globMatches("foo/**", "foo")).toBe(false);
    expect(globMatches("a/**/b", "a/b")).toBe(true);
    expect(globMatches("a/**/b", "a/x/y/b")).toBe(true);
    expect(globMatches("a/**/b", "ab")).toBe(false);
    expect(globMatches("**", "anything/at/all")).toBe(true);
    expect(globMatches("~/Workspace/**", "~/Workspace/a/b.txt")).toBe(true);
    expect(globMatches("~/Workspace/**", "~/Other/b.txt")).toBe(false);
  });

  it("treats ** inside a name as two stars", () => {
    expect(globMatches("a**b", "a/x/b")).toBe(true);
    expect(globProblem("a**b")).toBeNull();
  });

  it("supports alternates and classes", () => {
    expect(globMatches("mcp__{github,gitlab}__*", "mcp__gitlab__list")).toBe(true);
    expect(globMatches("mcp__{github,gitlab}__*", "mcp__jira__list")).toBe(false);
    expect(globMatches("file[0-9].txt", "file7.txt")).toBe(true);
    expect(globMatches("file[0-9].txt", "filex.txt")).toBe(false);
    expect(globMatches("file[!0-9].txt", "filex.txt")).toBe(true);
    expect(globMatches("file[^0-9].txt", "file7.txt")).toBe(false);
    expect(globMatches("[]]x", "]x")).toBe(true);
    expect(globMatches("a\\*b", "a*b")).toBe(true);
    expect(globMatches("a\\*b", "axb")).toBe(false);
    expect(globMatches("a.b", "axb")).toBe(false);
  });

  it("agrees with globset on the corner cases (checked against the real crate)", () => {
    expect(globMatches("{a,**/b}", "x/b")).toBe(true);
    expect(globMatches("foo{,.txt}", "foo.txt")).toBe(true);
    expect(globMatches("foo{,.txt}", "foo")).toBe(false);
    expect(globMatches("[a-]x", "-x")).toBe(true);
    expect(globMatches("**foo", "a/foo")).toBe(true);
    expect(globMatches("a/**b", "a/xb")).toBe(true);
    expect(globMatches("{a,b}/**", "a/x")).toBe(true);
    expect(globMatches("x/{a,b}/**", "x/b/y")).toBe(true);
    expect(globMatches("{a,b/**}", "b/z")).toBe(true);
    expect(globMatches("a/**/", "a/x/")).toBe(true);
  });

  it("falls back to plain text for patterns the crate rejects", () => {
    for (const bad of ["[abc", "{a,b", "a}", "a\\", "[z-a]"]) expect(globProblem(bad)).not.toBeNull();
    expect(globMatches("[abc", "[abc")).toBe(true);
    expect(globMatches("[abc", "a")).toBe(false);
  });
});

const rule = (tool: string, pattern: string | null, action: PolicyRule["action"]): PolicyRule => ({ tool, pattern, action });

describe("rule matching", () => {
  it("needs a subject when the rule has a pattern", () => {
    expect(ruleMatches(rule("shell", "git *", "allow"), "shell", null)).toBe(false);
    expect(ruleMatches(rule("shell", "git *", "allow"), "shell", "git status")).toBe(true);
    expect(ruleMatches(rule("shell", null, "allow"), "shell", null)).toBe(true);
  });

  it("matches a trailing-star pattern by prefix as the backend does", () => {
    expect(ruleMatches(rule("shell", "git status*", "allow"), "shell", "git status --short")).toBe(true);
    expect(ruleMatches(rule("shell", "git status*", "allow"), "shell", "git stash")).toBe(false);
  });

  it("labels rules the way they are written", () => {
    expect(ruleLabel(rule("shell", "rm *", "deny"))).toBe("shell(rm *)");
    expect(ruleLabel(rule("write_file", null, "ask"))).toBe("write_file");
  });
});

describe("evaluate (mirror of policy.rs)", () => {
  it("maps risks to decisions in every mode", () => {
    const kind = (mode: string, tool: string, risk: Parameters<typeof evaluate>[0]["risk"], extra = {}) =>
      evaluate({ mode, rules: [], tool, risk, ...extra }).kind;
    expect(kind("ask", "read_file", "read")).toBe("allow");
    expect(kind("ask", "write_file", "write")).toBe("ask");
    expect(kind("plan", "shell", "exec")).toBe("deny");
    expect(kind("plan", "web_fetch", "network")).toBe("allow");
    expect(kind("auto-edit", "edit_file", "write")).toBe("allow");
    expect(kind("auto-edit", "shell", "exec")).toBe("ask");
    expect(kind("auto", "shell", "exec")).toBe("allow");
    expect(kind("auto", "shell", "exec", { sandboxAvailable: false })).toBe("ask");
    expect(kind("auto", "computer_click", "computer")).toBe("ask");
    expect(kind("yolo", "computer_click", "computer")).toBe("allow");
    expect(kind("ask", "write_file", "write", { sessionAllowed: true })).toBe("allow");
    expect(kind("boh", "write_file", "write")).toBe("ask");
  });

  const rules = [rule("shell", "git status*", "allow"), rule("shell", "rm *", "deny"), rule("mcp__github__*", null, "ask")];

  it("lets rules override the mode and deny win", () => {
    const d1 = evaluate({ mode: "ask", rules, tool: "shell", risk: "exec", subject: "git status --short" });
    expect(d1).toMatchObject({ kind: "allow", by: "rule", ruleIndex: 0 });
    const d2 = evaluate({ mode: "ask", rules, tool: "shell", risk: "exec", subject: "rm -rf /" });
    expect(d2).toMatchObject({ kind: "deny", by: "rule", ruleIndex: 1 });
    expect(d2.reason).toBe("regola: shell(rm *)");
    expect(evaluate({ mode: "ask", rules, tool: "shell", risk: "exec", subject: "rm -rf /", sessionAllowed: true }).kind).toBe("deny");
    expect(evaluate({ mode: "ask", rules, tool: "mcp__github__list_issues", risk: "read" })).toMatchObject({ kind: "ask", ruleIndex: 2 });
  });

  it("ignores the order of the rules: deny beats allow beats ask", () => {
    const both = [rule("shell", null, "allow"), rule("shell", "rm *", "deny"), rule("shell", null, "ask")];
    expect(evaluate({ mode: "ask", rules: both, tool: "shell", risk: "exec", subject: "rm x" }).kind).toBe("deny");
    expect(evaluate({ mode: "ask", rules: [...both].reverse(), tool: "shell", risk: "exec", subject: "rm x" }).kind).toBe("deny");
    expect(evaluate({ mode: "plan", rules: both, tool: "shell", risk: "exec", subject: "ls" }).kind).toBe("allow");
    expect(evaluate({ mode: "ask", rules: [rule("shell", null, "ask"), rule("shell", null, "allow")], tool: "shell", risk: "exec" }).kind).toBe("allow");
  });

  it("uses the first matching rule among equal actions", () => {
    const two = [rule("shell*", null, "deny"), rule("shell", null, "deny")];
    expect(evaluate({ mode: "ask", rules: two, tool: "shell", risk: "exec" }).ruleIndex).toBe(0);
  });
});

describe("rule tester", () => {
  const probe = { tool: "shell", subject: "", mode: "ask" as const, risk: "exec" as const, sessionAllowed: false, sandboxAvailable: true };
  const rules = [rule("shell", "git status*", "allow"), rule("shell", "rm *", "deny"), rule("shell*", null, "ask"), rule("", null, "deny")];

  it("says nothing without a tool", () => {
    expect(runProbe(rules, { ...probe, tool: "  " })).toBeNull();
  });

  it("reports the deciding rule and every rule that applies", () => {
    const r = runProbe(rules, { ...probe, subject: "git status -s" })!;
    expect(r.decision).toMatchObject({ kind: "allow", ruleIndex: 0 });
    expect(r.matches).toEqual([0, 2]);
  });

  it("falls back to the approval mode when no rule applies, and skips unfinished rules", () => {
    const r = runProbe(rules, { ...probe, tool: "write_file", risk: "write", subject: "a.txt" })!;
    expect(r.decision).toMatchObject({ kind: "ask", by: "mode" });
    expect(r.matches).toEqual([]);
  });

  it("lets a deny rule win over a matching allow", () => {
    const r = runProbe(rules, { ...probe, subject: "rm -rf x" })!;
    expect(r.decision).toMatchObject({ kind: "deny", ruleIndex: 1 });
  });
});

describe("rule list helpers", () => {
  it("moves items within bounds", () => {
    expect(moveItem([1, 2, 3], 0, 1)).toEqual([2, 1, 3]);
    expect(moveItem([1, 2, 3], 2, -1)).toEqual([1, 3, 2]);
    expect(moveItem([1, 2, 3], 0, -1)).toEqual([1, 2, 3]);
    expect(moveItem([1, 2, 3], 2, 5)).toEqual([1, 2, 3]);
  });

  it("flags empty tools as errors and bad globs or duplicates as warnings", () => {
    const issues = checkRules([rule("", null, "ask"), rule("shell", "[abc", "deny"), rule("read_file", null, "allow"), rule("read_file", null, "allow")], (g) => (g === "[abc" ? "classe [ non chiusa" : null));
    expect(issues.filter((i) => i.level === "error").map((i) => i.index)).toEqual([0]);
    expect(issues.filter((i) => i.level === "warn").map((i) => i.index)).toEqual([1, 3]);
  });

  it("normalizes what is sent to the backend", () => {
    expect(normalizeRules([{ tool: " shell ", pattern: "  ", action: "ask" }, { tool: "a", pattern: " b* ", action: "deny" }])).toEqual([
      { tool: "shell", pattern: null, action: "ask" },
      { tool: "a", pattern: "b*", action: "deny" },
    ]);
  });
});
