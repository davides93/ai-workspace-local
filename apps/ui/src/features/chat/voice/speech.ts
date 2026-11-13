import { t as tr } from "../../../shared/i18n";
/**
 * Text side of reading a reply aloud, pure: Markdown to text that reads well, sentence-sized
 * pieces, and the choice of the local voice.
 */

/** The preferred voice by name, else the default (or first) one for the language. */
export function pickVoice<V extends { name: string; lang: string; default?: boolean }>(voices: V[], language: string, preferred = ""): V | null {
  const byName = preferred && voices.find((v) => v.name === preferred);
  if (byName) return byName;
  const lang = language.toLowerCase().slice(0, 2);
  const same = voices.filter((v) => v.lang.toLowerCase().replace("_", "-").startsWith(lang));
  return same.find((v) => v.default) ?? same[0] ?? null;
}

/** Markdown → text that reads well aloud (tables become "cell, cell." lines). */
export function plainText(md: string): string {
  const lines = md
    .replace(/```[\s\S]*?```/g, `\n${tr("chat.codice_omesso")}\n`)
    .split("\n")
    .flatMap((line) => {
      const t = line.trim();
      if (t.includes("-") && /^\|?[\s:|-]+\|?$/.test(t)) return []; // table separator, rule
      if (/^([*_])(\s*\1){2,}$/.test(t)) return []; // *** or ___ rule
      if (t.split("|").length > 2) {
        const cells = t.replace(/^\||\|$/g, "").split("|").map((c) => c.trim()).filter(Boolean);
        return cells.length ? [`${cells.join(", ")}.`] : [];
      }
      return [line];
    });
  return lines
    .join("\n")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}#{1,6}\s+(.*)$/gm, "$1.")
    .replace(/^\s*[-*+]\s+/gm, "")
    .replace(/^\s*>\s?/gm, "")
    .replace(/(\*\*|__|~~|\*|_)(\S(?:.*?\S)?)\1/g, "$2")
    .replace(/https?:\/\/\S+/g, "link")
    .replace(/([.!?;:])\.+/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Sentence-sized pieces (long utterances get cut off by some engines). Splits only where the
 * punctuation is followed by a space, so "INDEX.md" or "3.5" stay whole.
 */
export function chunks(text: string, max = 200): string[] {
  const out: string[] = [];
  let cur = "";
  for (const s of text.replace(/([.!?;])\s+/g, "$1\u0000").split("\u0000")) {
    let piece = s.trim();
    while (piece.length > max) {
      const cut = piece.lastIndexOf(" ", max);
      const at = cut > 40 ? cut : max;
      if (cur) out.push(cur), (cur = "");
      out.push(piece.slice(0, at));
      piece = piece.slice(at).trim();
    }
    if (!piece) continue;
    if (cur && cur.length + piece.length + 1 > max) out.push(cur), (cur = "");
    cur = cur ? `${cur} ${piece}` : piece;
  }
  if (cur) out.push(cur);
  return out;
}
