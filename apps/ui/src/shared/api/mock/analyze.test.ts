import { describe, expect, it } from "vitest";
import { analyzeCommand } from "./analyze";

describe("demo command analysis", () => {
  it("splits a compound line into simple commands", () => {
    expect(analyzeCommand("git status && rm -rf x; ls | wc -l").commands).toEqual(["git status", "rm -rf x", "ls", "wc -l"]);
  });
  it("does not split inside quotes", () => {
    const a = analyzeCommand("echo 'a; b' && ls");
    expect(a.commands).toEqual(["echo a; b", "ls"]);
    expect(a.confident).toBe(true);
  });
  it("gives up on shapes it does not understand", () => {
    for (const line of ["echo $(rm x)", "echo `rm x`", "eval foo", "sh -c 'ls'", "cat <<EOF"]) expect(analyzeCommand(line).confident).toBe(false);
  });
});
