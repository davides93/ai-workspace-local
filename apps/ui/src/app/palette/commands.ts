export interface Command {
  id: string;
  label: string;
  /** Secondary text (group of the section, shortcut…). */
  hint?: string;
  group: "Vai a" | "Azioni";
  icon: string;
  keywords: string[];
  run: () => void;
}

/** Lowercase and strip accents so "attivita" finds "Attività". */
export function normalize(s: string): string {
  return s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
}

/** Commands matching every word of the query; label matches rank before keyword matches. */
export function filterCommands(commands: Command[], query: string): Command[] {
  const words = normalize(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return commands;
  const scored: { c: Command; score: number }[] = [];
  for (const c of commands) {
    const label = normalize(c.label);
    const hay = `${label} ${normalize(c.keywords.join(" "))} ${normalize(c.hint ?? "")}`;
    if (!words.every((w) => hay.includes(w))) continue;
    const score = words.every((w) => label.startsWith(w) || label.includes(` ${w}`)) ? 0 : words.every((w) => label.includes(w)) ? 1 : 2;
    scored.push({ c, score });
  }
  // Stable sort: equal scores keep the original order (routes first, then actions).
  return scored.sort((a, b) => a.score - b.score).map((x) => x.c);
}
