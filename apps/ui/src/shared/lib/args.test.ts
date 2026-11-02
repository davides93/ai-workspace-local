import { describe, expect, it } from "vitest";
import { parseArgs, prettyJson, summarizeArgs } from "./args";

describe("tool arguments", () => {
  it("parses JSON text and leaves a half-written call as text", () => {
    expect(parseArgs('{"a":1}')).toEqual({ a: 1 });
    expect(parseArgs("")).toEqual({});
    expect(parseArgs('{"command":')).toBe('{"command":');
  });
  it("indents JSON for display", () => {
    expect(prettyJson('{"a":1}')).toBe('{\n  "a": 1\n}');
    expect(prettyJson({ a: [1, 2] })).toBe('{\n  "a": [\n    1,\n    2\n  ]\n}');
    expect(prettyJson("plain text")).toBe("plain text");
  });
  it("summarizes on one line as key=value", () => {
    expect(summarizeArgs('{"path":"~/Downloads","pattern":"*.pdf"}')).toBe("path=~/Downloads  pattern=*.pdf");
    expect(summarizeArgs({ command: "ls -la", cwd: "~", flags: [1, 2] })).toBe("command=ls -la  cwd=~  flags=[1,2]");
  });
  it("flattens newlines and cuts long values", () => {
    expect(summarizeArgs({ content: "a\nb\n\nc" })).toBe("content=a ⏎ b ⏎ c");
    const s = summarizeArgs({ content: "x".repeat(500) }, 40);
    expect(s).toHaveLength(40);
    expect(s.endsWith("…")).toBe(true);
  });
  it("copes with empty and odd values", () => {
    expect(summarizeArgs(undefined)).toBe("");
    expect(summarizeArgs(null)).toBe("");
    expect(summarizeArgs(42)).toBe("42");
    expect(summarizeArgs([1, "a"])).toBe('[1,"a"]');
    expect(summarizeArgs("")).toBe("");
  });
});
