// Pure helpers behind the tag inputs: how a typed or pasted line becomes a list of values.

/**
 * Splits a command line into arguments the way a shell does: whitespace separates, single or
 * double quotes keep spaces inside one argument and adjacent pieces join (`--name="a b"` →
 * `--name=a b`). The backslash is a plain character, so Windows paths survive
 * (`C:\Users\me\Work Space` needs quotes only for the space). An unterminated quote runs to the
 * end of the line instead of failing, so half-typed input never throws.
 */
export function splitShellWords(line: string): string[] {
  const words: string[] = [];
  let cur = "";
  let started = false;
  let quote: '"' | "'" | null = null;
  for (const ch of line) {
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      started = true;
    } else if (/\s/.test(ch)) {
      if (started) words.push(cur);
      cur = "";
      started = false;
    } else {
      cur += ch;
      started = true;
    }
  }
  if (started) words.push(cur);
  return words;
}

/** Inverse of {@link splitShellWords}: quotes only the arguments that need it. */
export function joinShellWords(words: string[]): string {
  return words.map(quoteWord).join(" ");
}

function quoteWord(w: string): string {
  if (w !== "" && !/[\s"']/.test(w)) return w;
  if (!w.includes('"')) return `"${w}"`;
  if (!w.includes("'")) return `'${w}'`;
  // Both kinds of quote inside: wrap every piece between two `"` in double quotes and put each
  // `"` in single quotes, so the shell-style concatenation gives the original text back.
  return w
    .split('"')
    .map((part) => (part === "" ? "" : `"${part}"`))
    .join(`'"'`);
}

/** Splits a list of plain values on whitespace, commas, semicolons and line breaks (domains, tags). */
export function splitList(raw: string): string[] {
  return raw.split(/[\s,;]+/).filter(Boolean);
}
