import { describe, expect, it } from "vitest";
import { parseBlocks, parseInline, parseMarkdown, safeHref, stableCut, type Block, type Inline } from "./markdownParse";

const text = (v: string): Inline => ({ t: "text", v });

describe("inline", () => {
  it("parses bold, italic, strike and code", () => {
    expect(parseInline("a **b** *c* ~~d~~ `e`")).toEqual([
      text("a "), { t: "strong", c: [text("b")] }, text(" "), { t: "em", c: [text("c")] }, text(" "),
      { t: "del", c: [text("d")] }, text(" "), { t: "code", v: "e" },
    ]);
  });
  it("nests emphasis in both directions and handles triple markers", () => {
    expect(parseInline("**a *b* c**")).toEqual([{ t: "strong", c: [text("a "), { t: "em", c: [text("b")] }, text(" c")] }]);
    expect(parseInline("*a **b** c*")).toEqual([{ t: "em", c: [text("a "), { t: "strong", c: [text("b")] }, text(" c")] }]);
    expect(parseInline("***x***")).toEqual([{ t: "strong", c: [{ t: "em", c: [text("x")] }] }]);
  });
  it("does not italicize snake_case or arithmetic", () => {
    expect(parseInline("file_name_here")).toEqual([text("file_name_here")]);
    expect(parseInline("2 * 3 * 4")).toEqual([text("2 * 3 * 4")]);
    expect(parseInline("_ok_")).toEqual([{ t: "em", c: [text("ok")] }]);
  });
  it("keeps formatting characters inside code spans literal", () => {
    expect(parseInline("`**x**` e ``a`b``")).toEqual([{ t: "code", v: "**x**" }, text(" e "), { t: "code", v: "a`b" }]);
  });
  it("honours backslash escapes", () => {
    expect(parseInline("\\*non corsivo\\*")).toEqual([text("*non corsivo*")]);
  });
  it("turns newlines into line breaks", () => {
    expect(parseInline("a\nb")).toEqual([text("a"), { t: "br" }, text("b")]);
  });
  it("links http, https and mailto only", () => {
    expect(parseInline("[sito](https://example.com/a?b=1)")).toEqual([{ t: "link", href: "https://example.com/a?b=1", c: [text("sito")] }]);
    expect(parseInline("[scrivi](mailto:a@b.it)")).toEqual([{ t: "link", href: "mailto:a@b.it", c: [text("scrivi")] }]);
    expect(parseInline("[x](javascript:alert(1))")).toEqual([text("x")]);
    expect(parseInline("[x](data:text/html;base64,AAAA)")).toEqual([text("x")]);
    expect(safeHref("  https://a.b/c ")).toBe("https://a.b/c");
    expect(safeHref("ftp://a.b")).toBeNull();
  });
  it("shows images as links, never as <img>", () => {
    expect(parseInline("![logo](https://example.com/x.png)")).toEqual([{ t: "link", href: "https://example.com/x.png", c: [text("logo")] }]);
    expect(parseInline("![x](javascript:1)")).toEqual([text("x")]);
  });
  it("auto-links bare URLs without their trailing punctuation", () => {
    expect(parseInline("vedi https://example.com/a.")).toEqual([text("vedi "), { t: "link", href: "https://example.com/a", c: [text("https://example.com/a")] }, text(".")]);
    expect(parseInline("(https://example.com/a)")).toEqual([text("("), { t: "link", href: "https://example.com/a", c: [text("https://example.com/a")] }, text(")")]);
    expect(parseInline("<https://example.com>")).toEqual([{ t: "link", href: "https://example.com", c: [text("https://example.com")] }]);
  });
  it("never passes HTML through", () => {
    const nodes = parseInline("<script>alert(1)</script> <img src=x onerror=alert(1)> <b>x</b>");
    expect(nodes.every((n) => n.t === "text")).toBe(true);
    expect(nodes.map((n) => (n.t === "text" ? n.v : "")).join("")).toContain("<script>alert(1)</script>");
  });
  it("leaves a huge run of text unformatted instead of scanning it for every marker", () => {
    const nodes = parseInline(`**a ${"x ".repeat(5000)}**`);
    expect(nodes.every((n) => n.t === "text")).toBe(true);
    expect(parseInline(`${"y\n".repeat(9000)}`).filter((n) => n.t === "br").length).toBe(9000);
  });
  it("decodes the common entities to text", () => {
    expect(parseInline("a &lt; b &amp; c")).toEqual([text("a < b & c")]);
  });
});

describe("blocks", () => {
  const kinds = (bs: Block[]) => bs.map((b) => b.t);

  it("separates headings by level (h1-h3, deeper collapses to 3)", () => {
    const bs = parseBlocks("# Uno\n## Due\n### Tre\n#### Quattro");
    expect(bs.map((b) => (b.t === "h" ? b.level : 0))).toEqual([1, 2, 3, 3]);
  });
  it("needs a space after the hashes", () => {
    expect(kinds(parseBlocks("#hashtag"))).toEqual(["p"]);
  });
  it("splits paragraphs on blank lines and keeps soft breaks inside", () => {
    const bs = parseBlocks("riga uno\nriga due\n\nsecondo");
    expect(kinds(bs)).toEqual(["p", "p"]);
    expect((bs[0] as Extract<Block, { t: "p" }>).c).toEqual([text("riga uno"), { t: "br" }, text("riga due")]);
  });
  it("reads a code fence with its language and keeps its content verbatim", () => {
    const bs = parseBlocks("```ts\nconst a = **1**;\n\n  indent\n```\ndopo");
    expect(bs[0]).toEqual({ t: "code", lang: "ts", v: "const a = **1**;\n\n  indent" });
    expect(kinds(bs)).toEqual(["code", "p"]);
  });
  it("accepts tilde fences, longer fences and an unclosed fence (streaming)", () => {
    expect(parseBlocks("~~~\nx\n~~~")[0]).toMatchObject({ t: "code", v: "x" });
    expect(parseBlocks("````md\n```\ninner\n```\n````")[0]).toMatchObject({ t: "code", lang: "md", v: "```\ninner\n```" });
    expect(parseBlocks("```py\nprint(1)")[0]).toMatchObject({ t: "code", lang: "py", v: "print(1)" });
  });
  it("sanitizes the language label", () => {
    expect(parseBlocks("```<script>\nx\n```")[0]).toMatchObject({ lang: "script" });
  });
  it("parses bullet lists with nesting", () => {
    const [l] = parseBlocks("- a\n  - a1\n  - a2\n- b") as Extract<Block, { t: "list" }>[];
    expect(l.ordered).toBe(false);
    expect(l.items).toHaveLength(2);
    const nested = l.items[0].blocks[1] as Extract<Block, { t: "list" }>;
    expect(nested.t).toBe("list");
    expect(nested.items).toHaveLength(2);
    expect(l.tight).toBe(true);
  });
  it("nests lists indented by two spaces under an ordered item", () => {
    const [l] = parseBlocks("1. uno\n  - x\n2. due") as Extract<Block, { t: "list" }>[];
    expect(l.items).toHaveLength(2);
    expect(l.items[0].blocks.map((b) => b.t)).toEqual(["p", "list"]);
  });
  it("keeps the start number of ordered lists", () => {
    const [l] = parseBlocks("3. c\n4. d") as Extract<Block, { t: "list" }>[];
    expect(l).toMatchObject({ ordered: true, start: 3 });
    expect(l.items).toHaveLength(2);
  });
  it("keeps a loose list together and marks it loose", () => {
    const [l] = parseBlocks("1. a\n\n2. b\n\n3. c") as Extract<Block, { t: "list" }>[];
    expect(l.items).toHaveLength(3);
    expect(l.tight).toBe(false);
  });
  it("puts an indented paragraph and a code fence inside the list item", () => {
    const [l] = parseBlocks("- passo\n\n  dettaglio\n\n  ```sh\n  ls\n  ```\n- fine") as Extract<Block, { t: "list" }>[];
    expect(l.items[0].blocks.map((b) => b.t)).toEqual(["p", "p", "code"]);
    expect(l.items[0].blocks[2]).toMatchObject({ lang: "sh", v: "ls" });
    expect(l.items).toHaveLength(2);
  });
  it("reads task list items", () => {
    const [l] = parseBlocks("- [x] fatto\n- [ ] da fare") as Extract<Block, { t: "list" }>[];
    expect(l.items.map((i) => i.checked)).toEqual([true, false]);
  });
  it("ends a list at the first unrelated paragraph", () => {
    expect(kinds(parseBlocks("- a\n- b\n\nfine"))).toEqual(["list", "p"]);
  });
  it("interrupts a paragraph with a list", () => {
    expect(kinds(parseBlocks("Ecco:\n1. uno\n2. due"))).toEqual(["p", "list"]);
  });
  it("parses blockquotes recursively", () => {
    const [q] = parseBlocks("> citazione\n> - voce\n>\n> **fine**") as Extract<Block, { t: "quote" }>[];
    expect(q.t).toBe("quote");
    expect(q.c.map((b) => b.t)).toEqual(["p", "list", "p"]);
  });
  it("reads horizontal rules but not list bullets", () => {
    expect(kinds(parseBlocks("sopra\n\n---\n\nsotto\n\n***\n\n- - -"))).toEqual(["p", "hr", "p", "hr", "hr"]);
    expect(kinds(parseBlocks("- a"))).toEqual(["list"]);
  });
  it("parses GFM tables with alignment and escaped pipes", () => {
    const [t] = parseBlocks("| Anno | File | Note |\n|:---|---:|:---:|\n| 2024 | 2 | a \\| b |\n| 2025 | 3 |") as Extract<Block, { t: "table" }>[];
    expect(t.t).toBe("table");
    expect(t.align).toEqual(["left", "right", "center"]);
    expect(t.head).toHaveLength(3);
    expect(t.rows).toHaveLength(2);
    expect(t.rows[0][2]).toEqual([text("a | b")]);
    expect(t.rows[1]).toHaveLength(3);
  });
  it("does not take a lone pipe line for a table", () => {
    expect(kinds(parseBlocks("a | b\nc | d"))).toEqual(["p"]);
  });
  it("survives pathological nesting and empty input", () => {
    expect(parseBlocks("")).toEqual([]);
    const deep = `${"> ".repeat(40)}x`;
    expect(parseBlocks(deep).length).toBeGreaterThan(0);
    const lists = Array.from({ length: 30 }, (_, i) => `${"  ".repeat(i)}- n${i}`).join("\n");
    expect(parseBlocks(lists).length).toBeGreaterThan(0);
  });
  it("normalizes CRLF and tabs", () => {
    expect(kinds(parseBlocks("- a\r\n\t- b\r\n"))).toEqual(["list"]);
  });
});

describe("caching and streaming", () => {
  const doc = [
    "# Titolo", "", "Un paragrafo con **grassetto**.", "", "- uno", "- due", "", "```ts", "const a = 1;", "", "const b = 2;", "```", "",
    "1. primo", "", "2. secondo", "", "> citazione", "", "| a | b |", "|---|---|", "| 1 | 2 |", "", "Fine.",
  ].join("\n");

  it("returns the same blocks as a direct parse", () => {
    expect(parseMarkdown(doc)).toEqual(parseBlocks(doc));
  });
  it("gives the same result for every prefix of a streaming message", () => {
    for (let n = 1; n <= doc.length; n += 7) {
      const part = doc.slice(0, n);
      expect(parseMarkdown(part)).toEqual(parseBlocks(part));
    }
  });
  it("reuses the blocks of the stable head while the tail grows", () => {
    const head = `${"Primo paragrafo lungo abbastanza da superare la soglia della cache incrementale. ".repeat(3)}\n\n${"Secondo paragrafo, completo e stabile. ".repeat(3)}\n\n`;
    const a = parseMarkdown(`${head}terzo par`);
    const b = parseMarkdown(`${head}terzo paragrafo in arrivo`);
    expect(a[0]).toBe(b[0]);
    expect(a[1]).toBe(b[1]);
    expect(a[2]).not.toBe(b[2]);
  });
  it("returns the cached array for identical text", () => {
    expect(parseMarkdown(doc)).toBe(parseMarkdown(doc));
  });
  it("does not cut inside a code fence or a list", () => {
    const code = `${"x".repeat(80)}\n\n\`\`\`\nuno\n\ndue\n\`\`\``;
    expect(stableCut(code)).toBe(code.indexOf("```"));
    const list = `${"x".repeat(80)}\n\n- a\n\n- b\n\n- c`;
    expect(stableCut(list)).toBe(0);
    expect(parseMarkdown(list)).toEqual(parseBlocks(list));
  });
});
