import { describe, expect, it } from "vitest";
import { GRAMMARS, grammarFor, type GrammarId } from "./languages";
import { MAX_HIGHLIGHT_CHARS, tokenize, type Kind } from "./tokenize";

const join = (code: string, g: GrammarId) => tokenize(code, g).map((t) => t.v).join("");
/** The text of the runs of one kind, in order. */
const of = (code: string, g: GrammarId, kind: Kind) => tokenize(code, g).filter((t) => t.k === kind).map((t) => t.v);

const SAMPLES: Record<GrammarId, string> = {
  js: "// uno\nconst a = { b: 'x', c: `t ${1}` };\nasync function f(x) { return await g(x ?? 0x1f) /* due */; }\nclass Foo extends Bar {}\n",
  ts: "@Component({})\nexport interface User { id: number; name?: string }\nconst u: Record<string, User> = {};\nfunction f<T>(x: T): T { return x }\n",
  json: '{\n  "name": "aiw", "n": -1.5e3, "ok": true, "x": null, "a": [1, 2], // c\n  "s": "a \\"b\\""\n}\n',
  python: '#!/usr/bin/env python\n@app.route("/")\ndef f(x, *, y=None):\n    """doc\n    string"""\n    s = f"a{x}" + \'b\' + r"\\d"\n    return True if x else 1_000\nclass A(B): pass\n',
  shell: '# build\nexport FOO="a $B" && cargo build --release -p aiw-core\nif [ -n "$X" ]; then echo \'ok\' ${HOME}; fi\n$ ls -la | grep foo\n',
  rust: '// c\n#[derive(Debug)]\npub fn main<\'a>(s: &\'a str) -> Result<(), String> {\n    let n: u32 = 0xff_u32; println!("{}", s);\n    let c = \'x\'; let r = r#"raw "q""#;\n    Ok(())\n}\nstruct Foo { x: i64 }\n',
  c: '#include <stdio.h>\n#define N 10\n/* c */\nint main(void) {\n  const char *s = "hi\\n"; // x\n  return sizeof(int) + 0x1F;\n}\n',
  java: '@Override\npublic class A extends B {\n  private final String s = "x"; // c\n  void f(int n) { if (n > 0) throw new Error(); }\n}\n',
  go: 'package main\n\nimport "fmt"\n\nfunc main() {\n\tx := `raw`\n\tfmt.Println("a", 1, nil, x) // c\n}\n',
  csharp: '[Serializable]\npublic class A : B {\n  public async Task<int> F(string s) { return await G(s) ?? 0; }\n}\n',
  css: "/* c */\n@media (min-width: 600px) {\n  .a > #b:hover { color: #fff; margin: -4px 1.5rem; --x: 1; background: url(\"a.png\") !important; }\n}\n",
  html: '<!doctype html>\n<!-- c -->\n<div class="a" id=\'b\' data-x=1 hidden>\n  He said "hi" &amp; left <br/>\n</div>\n',
  sql: "-- c\nSELECT u.id, count(*) FROM users u JOIN t ON t.id = u.id WHERE name = 'a''b' AND n > 10 GROUP BY u.id;\nCREATE TABLE x (id serial PRIMARY KEY, n text);\n",
  yaml: "# c\nname: aiw\nitems:\n  - id: 1\n    ok: true\n    url: http://x/y\n  - &a \"q\"\n",
  toml: '# c\n[package]\nname = "aiw"\nversion = "0.5.0"\nbuilt = 2026-11-15T10:00:00Z\n[[bin]]\nok = true\n',
  diff: "diff --git a/a b/a\nindex 1..2 100644\n--- a/a\n+++ b/a\n@@ -1,3 +1,3 @@ fn x\n context a - b\n-old\n+new\n",
};

describe("tokenize", () => {
  it("has a sample for every grammar", () => {
    expect(Object.keys(SAMPLES).sort()).toEqual([...GRAMMARS].sort());
  });

  for (const g of GRAMMARS) {
    it(`${g}: the runs joined are the input, and some are coloured`, () => {
      const toks = tokenize(SAMPLES[g], g);
      expect(toks.map((t) => t.v).join("")).toBe(SAMPLES[g]);
      expect(toks.some((t) => t.k !== null)).toBe(true);
      // Adjacent runs of the same kind are merged, and no run is empty.
      for (let i = 0; i < toks.length; i++) {
        expect(toks[i].v.length).toBeGreaterThan(0);
        if (i > 0) expect(toks[i].k).not.toBe(toks[i - 1].k);
      }
    });
  }

  it("js: comments, strings, numbers, keywords, literals, functions and types", () => {
    const code = SAMPLES.js;
    expect(of(code, "js", "com")).toEqual(["// uno", "/* due */"]);
    expect(of(code, "js", "str")).toEqual(["'x'", "`t ${1}`"]);
    expect(of(code, "js", "num")).toEqual(["0x1f"]);
    expect(of(code, "js", "kw")).toEqual(expect.arrayContaining(["const", "async", "function", "return", "await", "class", "extends"]));
    expect(of(code, "js", "fn")).toEqual(expect.arrayContaining(["f", "g"]));
    expect(of(code, "js", "ty")).toEqual(expect.arrayContaining(["Foo", "Bar"]));
  });

  it("a keyword inside a string or a comment is not a keyword", () => {
    expect(of("const s = 'if else return'; // for while", "js", "kw")).toEqual(["const"]);
    expect(of("x = 1 # if else", "python", "kw")).toEqual([]);
  });

  it("ts: decorators and type names", () => {
    expect(of(SAMPLES.ts, "ts", "attr")).toEqual(["@Component"]);
    expect(of(SAMPLES.ts, "ts", "kw")).toEqual(expect.arrayContaining(["export", "interface", "const", "function", "return"]));
    expect(of(SAMPLES.ts, "ts", "ty")).toEqual(expect.arrayContaining(["number", "string", "Record", "User"]));
  });

  it("json: keys are properties, values are strings, numbers and literals", () => {
    expect(of(SAMPLES.json, "json", "attr")).toEqual(['"name"', '"n"', '"ok"', '"x"', '"a"', '"s"']);
    expect(of(SAMPLES.json, "json", "str")).toEqual(['"aiw"', '"a \\"b\\""']);
    expect(of(SAMPLES.json, "json", "num")).toEqual(["-1.5e3", "1", "2"]);
    expect(of(SAMPLES.json, "json", "lit")).toEqual(["true", "null"]);
  });

  it("python: triple-quoted and prefixed strings, decorators, literals", () => {
    const strs = of(SAMPLES.python, "python", "str");
    expect(strs[0]).toBe('"/"');
    expect(strs).toContain('"""doc\n    string"""');
    expect(strs).toEqual(expect.arrayContaining(['f"a{x}"', "'b'", 'r"\\d"']));
    expect(of(SAMPLES.python, "python", "attr")).toEqual(['@app.route']);
    expect(of(SAMPLES.python, "python", "lit")).toEqual(["None", "True"]);
    expect(of(SAMPLES.python, "python", "com")).toEqual(["#!/usr/bin/env python"]);
  });

  it("shell: variables, flags, strings and a comment only where a comment can start", () => {
    expect(of(SAMPLES.shell, "shell", "com")).toEqual(["# build"]);
    expect(of(SAMPLES.shell, "shell", "attr")).toEqual(["${HOME}"]);
    expect(of(SAMPLES.shell, "shell", "lit")).toEqual(["--release", "-p", "-n", "-la"]);
    expect(of("echo a#b # c", "shell", "com")).toEqual(["# c"]);
    expect(of(SAMPLES.shell, "shell", "kw")).toEqual(expect.arrayContaining(["if", "then", "fi"]));
    // A variable in a double-quoted string stays part of the string; on its own it is coloured.
    expect(of('echo "a $B" $C', "shell", "attr")).toEqual(["$C"]);
  });

  it("rust: attributes, lifetimes, char and raw strings, macros", () => {
    const code = SAMPLES.rust;
    expect(of(code, "rust", "attr")).toEqual(["#[derive(Debug)]", "'a", "'a"]);
    expect(of(code, "rust", "fn")).toEqual(expect.arrayContaining(["println!", "main"]));
    expect(of(code, "rust", "str")).toEqual(expect.arrayContaining(['"{}"', "'x'", 'r#"raw "q""#']));
    expect(of(code, "rust", "num")).toEqual(["0xff_u32"]);
  });

  it("c: preprocessor directives, comments, strings", () => {
    expect(of(SAMPLES.c, "c", "kw")).toEqual(expect.arrayContaining(["#include", "#define", "return", "const", "sizeof"]));
    expect(of(SAMPLES.c, "c", "com")).toEqual(["/* c */", "// x"]);
  });

  it("go: raw strings and nil", () => {
    expect(of(SAMPLES.go, "go", "str")).toEqual(['"fmt"', "`raw`", '"a"']);
    expect(of(SAMPLES.go, "go", "lit")).toEqual(["nil"]);
  });

  it("css: at-rules, properties, selectors, colours and units", () => {
    const c = SAMPLES.css;
    expect(of(c, "css", "kw")).toEqual(["@media", "!important"]);
    expect(of(c, "css", "attr")).toEqual(expect.arrayContaining(["color", "margin", "--x", "background"]));
    expect(of(c, "css", "ty")).toEqual([".a", "#b"]);
    expect(of(c, "css", "num")).toEqual(expect.arrayContaining(["600px", "#fff", "-4px", "1.5rem", "1"]));
  });

  it("html: tags, attributes and values; the text between tags is not read as strings", () => {
    const h = SAMPLES.html;
    expect(of(h, "html", "ty")).toEqual(["<div", "<br", "</div"]);
    expect(of(h, "html", "attr")).toEqual(["class", "id", "data-x", "hidden"].slice(0, 3));
    expect(of(h, "html", "str")).toEqual(['"a"', "'b'"]);
    expect(of(h, "html", "lit")).toEqual(["&amp;"]);
    expect(of(h, "html", "com")).toEqual(expect.arrayContaining(["<!-- c -->"]));
  });

  it("sql: keywords in any case, quoted strings with doubled quotes", () => {
    expect(of(SAMPLES.sql, "sql", "str")).toEqual(["'a''b'"]);
    expect(of("select 1 FROM t Where x", "sql", "kw")).toEqual(["select", "FROM", "Where"]);
    expect(of(SAMPLES.sql, "sql", "fn")).toEqual(["count"]);
    expect(of("CREATE TABLE x (id int)", "sql", "fn")).toEqual([]);
  });

  it("yaml and toml: keys, sections and dates", () => {
    expect(of(SAMPLES.yaml, "yaml", "attr")).toEqual(["name", "items", "id", "ok", "url"]);
    expect(of(SAMPLES.yaml, "yaml", "lit")).toEqual(["true"]);
    expect(of(SAMPLES.toml, "toml", "ty")).toEqual(["[package]", "[[bin]]"]);
    expect(of(SAMPLES.toml, "toml", "num")).toEqual(["2026-11-15T10:00:00Z"]);
  });

  it("diff: added, removed and header lines; a context line is plain whatever it holds", () => {
    expect(of(SAMPLES.diff, "diff", "ins")).toEqual(["+new"]);
    expect(of(SAMPLES.diff, "diff", "del")).toEqual(["-old"]);
    expect(of(SAMPLES.diff, "diff", "kw")).toEqual(["@@ -1,3 +1,3 @@ fn x"]);
    expect(of(SAMPLES.diff, "diff", "com")).toHaveLength(4);
    expect(tokenize(" context a - b\n", "diff").every((t) => t.k === null)).toBe(true);
  });

  it("a string or comment that is not finished (a block still streaming) runs to the end", () => {
    expect(tokenize("const a = 'abc", "js")).toEqual([{ k: "kw", v: "const" }, { k: null, v: " a = " }, { k: "str", v: "'abc" }]);
    expect(tokenize("x /* never closed\nmore", "js").at(-1)).toEqual({ k: "com", v: "/* never closed\nmore" });
    expect(tokenize('s = """open\nlines', "python").at(-1)).toEqual({ k: "str", v: '"""open\nlines' });
  });

  it("empty and oversized input come back plain", () => {
    expect(tokenize("", "js")).toEqual([{ k: null, v: "" }]);
    const big = "const a = 1;\n".repeat(Math.ceil(MAX_HIGHLIGHT_CHARS / 12) + 1);
    expect(big.length).toBeGreaterThan(MAX_HIGHLIGHT_CHARS);
    expect(tokenize(big, "js")).toEqual([{ k: null, v: big }]);
  });

  it("keeps every character for any input: a seeded random sweep over every grammar", () => {
    let seed = 20261115;
    const rnd = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 2 ** 32;
    };
    const alphabet = ["\"", "'", "`", "/", "*", "\\", "\n", "\n", " ", "\t", "#", "<", ">", "!", "-", "+", "@", "$", "{", "}", "(", ")", "[", "]", "=", ":", ";", "&", "a", "B", "z9", "0x", "1.5", "if", "r\"", "<a ", "--", "/*", "*/", "\"\"\"", "é", "😀", "\r"];
    for (let n = 0; n < 300; n++) {
      const len = 1 + Math.floor(rnd() * 120);
      let s = "";
      for (let i = 0; i < len; i++) s += alphabet[Math.floor(rnd() * alphabet.length)];
      for (const g of GRAMMARS) expect(join(s, g), `${g}: ${JSON.stringify(s)}`).toBe(s);
    }
  });

  it("does not blow up on inputs built to make a regex backtrack: each one, in every grammar, takes a fraction of a second", () => {
    // A greedy run followed by a lookahead is quadratic when it is tried at every position of the run; these are the runs the rules
    // of every grammar have (words with `.`, `-`, `/`, `@`, `=`; tags; strings and comments that never close), at the size of the cap.
    const cap = MAX_HIGHLIGHT_CHARS;
    const nasty: [string, string][] = [
      ["<a ", "<a ".repeat(cap / 3)], ["quotes", '"'.repeat(cap)], ["comments", "/*".repeat(cap / 2)], ["escapes", "'\\".repeat(cap / 2)],
      ["attributes", "<a b=".repeat(cap / 5)], ["brackets", "[".repeat(cap)], ["keys", "a:".repeat(cap / 2)], ["hashes", "\n#".repeat(cap / 2)],
      ["hunks", "@@".repeat(cap / 2)], ["zeros", `${"0".repeat(cap - 1)}x`],
      ["a.", "a.".repeat(cap / 2)], ["a-", "a-".repeat(cap / 2)], ["a/", "a/".repeat(cap / 2)], ["a@", "a@".repeat(cap / 2)], ["x=", "x=".repeat(cap / 2)],
      ["dashes", "-".repeat(cap)], ["slashes", "/".repeat(cap)], ["dots", ".".repeat(cap)],
      ["long attribute name", `<a ${"x".repeat(cap - 3)}`], ["attribute names with dashes", `<a ${"x-".repeat(cap / 2 - 2)}`], ["values", `<a b=${"c=".repeat(cap / 2 - 3)}`],
      ["css words", `${"a-".repeat(cap / 2 - 1)}:`], ["css functions", `${"a-".repeat(cap / 2 - 1)}(`], ["yaml key", `${"a.".repeat(cap / 2 - 1)}:`], ["toml key", `${"a.".repeat(cap / 2 - 1)} =`],
      ["many short lines", "k: v\n".repeat(cap / 5)],
    ];
    const slow: string[] = [];
    for (const [name, text] of nasty) {
      const input = text.slice(0, cap);
      for (const g of GRAMMARS) {
        const t0 = performance.now();
        const out = join(input, g);
        const ms = performance.now() - t0;
        expect(out.length, `${g} ${name}`).toBe(input.length);
        // Linear, the slowest of these takes some tens of milliseconds; the quadratic ones took one to three seconds.
        if (ms > 600) slow.push(`${g} · ${name}: ${Math.round(ms)} ms`);
      }
    }
    expect(slow).toEqual([]);
  });
});

describe("grammarFor", () => {
  it("maps the names a fence can carry, whatever their case, and nothing else", () => {
    expect(grammarFor("TypeScript")).toBe("ts");
    expect(grammarFor("tsx")).toBe("ts");
    expect(grammarFor("JS")).toBe("js");
    expect(grammarFor("bash")).toBe("shell");
    expect(grammarFor("c++")).toBe("c");
    expect(grammarFor("yml")).toBe("yaml");
    expect(grammarFor("py")).toBe("python");
    expect(grammarFor("")).toBeUndefined();
    expect(grammarFor(undefined)).toBeUndefined();
    expect(grammarFor("brainfuck")).toBeUndefined();
    expect(grammarFor("constructor")).toBeUndefined();
    expect(grammarFor("__proto__")).toBeUndefined();
  });
});
