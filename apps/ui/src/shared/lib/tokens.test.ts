import { describe, expect, it } from "vitest";
import { joinShellWords, splitList, splitShellWords } from "./tokens";

describe("splitShellWords", () => {
  it("splits on whitespace", () => {
    expect(splitShellWords("npx -y  @modelcontextprotocol/server-filesystem")).toEqual(["npx", "-y", "@modelcontextprotocol/server-filesystem"]);
    expect(splitShellWords("  ")).toEqual([]);
    expect(splitShellWords("")).toEqual([]);
  });
  it("keeps spaces inside quotes", () => {
    expect(splitShellWords('--root "~/My Documents" --verbose')).toEqual(["--root", "~/My Documents", "--verbose"]);
    expect(splitShellWords("--root '~/My Documents'")).toEqual(["--root", "~/My Documents"]);
  });
  it("joins adjacent pieces like a shell", () => {
    expect(splitShellWords('--name="a b"')).toEqual(["--name=a b"]);
    expect(splitShellWords('say"hello world"now')).toEqual(["sayhello worldnow"]);
  });
  it("keeps the other quote kind literal inside quotes", () => {
    expect(splitShellWords(`"it's fine"`)).toEqual(["it's fine"]);
    expect(splitShellWords(`'say "hi"'`)).toEqual(['say "hi"']);
  });
  it("keeps empty quoted arguments", () => {
    expect(splitShellWords('a "" b')).toEqual(["a", "", "b"]);
  });
  it("treats the backslash as a plain character (Windows paths)", () => {
    expect(splitShellWords("C:\\Users\\me\\Work")).toEqual(["C:\\Users\\me\\Work"]);
    expect(splitShellWords('"C:\\Program Files\\x"')).toEqual(["C:\\Program Files\\x"]);
  });
  it("does not fail on an unterminated quote", () => {
    expect(splitShellWords('--root "~/My Docu')).toEqual(["--root", "~/My Docu"]);
  });
});

describe("joinShellWords", () => {
  it("quotes only what needs it", () => {
    expect(joinShellWords(["-y", "pkg", "~/My Documents", ""])).toBe('-y pkg "~/My Documents" ""');
    expect(joinShellWords(['say "hi" now'])).toBe("'say \"hi\" now'");
  });
  it("round-trips any list of arguments", () => {
    const cases = [
      ["a", "b c", "d"],
      ["--x=1", "C:\\Program Files\\y", "it's", 'q"uote'],
      ["has 'single' and \"double\" quotes"],
      ["", "tab\tseparated"],
    ];
    for (const words of cases) expect(splitShellWords(joinShellWords(words))).toEqual(words);
  });
});

describe("splitList", () => {
  it("splits on spaces, commas, semicolons and newlines", () => {
    expect(splitList("a.com, b.com;c.com\nd.com  e.com")).toEqual(["a.com", "b.com", "c.com", "d.com", "e.com"]);
    expect(splitList(" , ")).toEqual([]);
  });
});
