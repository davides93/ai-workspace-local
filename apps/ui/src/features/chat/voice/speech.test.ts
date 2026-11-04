import { describe, expect, it } from "vitest";
import { chunks, pickVoice, plainText } from "./speech";

const v = (name: string, lang: string, def = false) => ({ name, lang, default: def });

describe("pickVoice", () => {
  const voices = [v("Alice", "it-IT"), v("Luca", "it_IT", true), v("Sam", "en-US", true)];
  it("prefers the voice named in the settings", () => {
    expect(pickVoice(voices, "it", "Alice")?.name).toBe("Alice");
  });
  it("else the default voice for the language, else the first", () => {
    expect(pickVoice(voices, "it-IT")?.name).toBe("Luca");
    expect(pickVoice(voices, "EN")?.name).toBe("Sam");
    expect(pickVoice([v("A", "it-IT"), v("B", "it-IT")], "it")?.name).toBe("A");
  });
  it("returns null when no local voice speaks the language", () => {
    expect(pickVoice(voices, "de")).toBeNull();
    expect(pickVoice([], "it")).toBeNull();
    expect(pickVoice(voices, "it", "Inesistente")?.name).toBe("Luca");
  });
});

describe("plainText", () => {
  it("drops the Markdown syntax", () => {
    expect(plainText("## Titolo\n\n**Grassetto** e `codice` con [un link](https://x.it/a).")).toBe("Titolo. Grassetto e codice con un link.");
    expect(plainText("- uno\n- due\n> citazione")).toBe("uno due citazione");
    expect(plainText("vedi https://example.com/pagina ora")).toBe("vedi link ora");
  });
  it("reads a table as lines and skips its separator", () => {
    expect(plainText("| Nome | Età |\n|---|---|\n| Ada | 36 |")).toBe("Nome, Età. Ada, 36.");
  });
  it("omits code blocks", () => {
    expect(plainText("prima\n```js\nconst a = 1;\n```\ndopo")).toBe("prima (codice omesso) dopo");
  });
  it("drops horizontal rules and images", () => {
    expect(plainText("a\n\n---\n\n![logo](x.png)b")).toBe("a b");
  });
});

describe("chunks", () => {
  it("splits on sentence ends followed by a space and keeps names and numbers whole", () => {
    expect(chunks("Apro INDEX.md. Costa 3.5 euro! Va bene?", 20)).toEqual(["Apro INDEX.md.", "Costa 3.5 euro!", "Va bene?"]);
  });
  it("packs short sentences together up to the limit", () => {
    expect(chunks("Uno. Due. Tre.", 200)).toEqual(["Uno. Due. Tre."]);
  });
  it("cuts a sentence longer than the limit at a space", () => {
    const text = Array(60).fill("parola").join(" ");
    const out = chunks(text, 100);
    expect(out.length).toBeGreaterThan(3);
    expect(out.every((p) => p.length <= 100)).toBe(true);
    expect(out.join(" ")).toBe(text);
  });
  it("gives nothing for an empty text", () => {
    expect(chunks("")).toEqual([]);
    expect(chunks("   ")).toEqual([]);
  });
});
