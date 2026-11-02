import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Markdown } from "./Markdown";

const html = (text: string, streaming = false) => renderToStaticMarkup(createElement(Markdown, { text, streaming }));

describe("Markdown rendering", () => {
  it("never lets HTML through: tags in the source come out escaped", () => {
    const out = html('<script>alert(1)</script> <img src=x onerror=alert(1)> <a href="javascript:alert(1)">x</a>');
    expect(out).not.toContain("<script");
    expect(out).not.toContain("<img");
    expect(out).toContain("&lt;script&gt;");
  });
  it("does not make a link of a javascript: URL and opens real ones safely", () => {
    expect(html("[x](javascript:alert(1))")).not.toContain("<a ");
    const out = html("[sito](https://example.com)");
    expect(out).toContain('href="https://example.com"');
    expect(out).toContain('rel="noopener noreferrer"');
    expect(out).toContain('target="_blank"');
  });
  it("shows an image as a link, never as an <img>", () => {
    const out = html("![logo](https://example.com/x.png)");
    expect(out).not.toContain("<img");
    expect(out).toContain('href="https://example.com/x.png"');
  });
  it("renders the heading levels below the page's own (h3, h4, h5)", () => {
    const out = html("# Uno\n\n## Due\n\n### Tre");
    expect(out).toContain('<h3 class="md-h md-h1">Uno</h3>');
    expect(out).toContain('<h4 class="md-h md-h2">Due</h4>');
    expect(out).toContain('<h5 class="md-h md-h3">Tre</h5>');
  });
  it("renders emphasis, rules, quotes and nested lists", () => {
    const out = html("*it* **bo** ~~st~~\n\n---\n\n> cit\n\n- a\n  - b");
    expect(out).toContain("<em>it</em>");
    expect(out).toContain("<strong>bo</strong>");
    expect(out).toContain("<del>st</del>");
    expect(out).toContain("<hr/>");
    expect(out).toContain("<blockquote>");
    expect(out).toMatch(/<ul><li>a<ul><li>b<\/li><\/ul><\/li><\/ul>/);
  });
  it("keeps the start number of an ordered list", () => {
    expect(html("3. c\n4. d")).toContain('<ol start="3">');
  });
  it("labels a code block with its language and gives it a copy button", () => {
    const out = html("```ts\nconst a = 1;\n```");
    expect(out).toContain('class="codeblock"');
    expect(out).toContain(">ts<");
    expect(out).toContain("Copia");
    expect(out).toContain("const a = 1;");
  });
  it("renders a task list with read-only checkboxes that say their state", () => {
    const out = html("- [x] fatto\n- [ ] da fare");
    expect(out).toContain('aria-label="Completato"');
    expect(out).toContain('aria-label="Da fare"');
    expect(out).toContain("disabled");
  });
  it("renders a table with alignment classes inside a scroll wrapper", () => {
    const out = html("| a | b |\n|:-:|--:|\n| 1 | 2 |");
    expect(out).toContain('class="md-table"');
    expect(out).toContain("md-al-center");
    expect(out).toContain("md-al-right");
  });
  it("marks the open message while it streams", () => {
    expect(html("ciao", true)).toContain("is-streaming");
    expect(html("ciao", false)).not.toContain("is-streaming");
    expect(html("", true)).toContain("is-streaming");
  });
});
