import { normalizeText } from "../../shared/lib/text";

export interface Command {
  id: string;
  label: string;
  /** Secondary text (group of the section, shortcut…). */
  hint?: string;
  group: "go" | "actions";
  icon: string;
  keywords: string[];
  run: () => void;
}

/** Commands matching every word of the query; label matches rank before keyword matches. */
export function filterCommands(commands: Command[], query: string): Command[] {
  const words = normalizeText(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return commands;
  const scored: { c: Command; score: number }[] = [];
  for (const c of commands) {
    const label = normalizeText(c.label);
    const hay = `${label} ${normalizeText(c.keywords.join(" "))} ${normalizeText(c.hint ?? "")}`;
    if (!words.every((w) => hay.includes(w))) continue;
    const score = words.every((w) => label.startsWith(w) || label.includes(` ${w}`)) ? 0 : words.every((w) => label.includes(w)) ? 1 : 2;
    scored.push({ c, score });
  }
  // Stable sort: equal scores keep the original order (routes first, then actions).
  return scored.sort((a, b) => a.score - b.score).map((x) => x.c);
}

/** Keywords of a command as written in a message: comma separated words or phrases. */
export const splitKeywords = (list: string): string[] => list.split(",").map((w) => w.trim()).filter(Boolean);
