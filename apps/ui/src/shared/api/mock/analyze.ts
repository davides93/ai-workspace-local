// Demo stand-in for `POST /api/policy/analyze` (the backend's lexer is crates/aiw-agent/src/shellparse.rs).
// It is deliberately simpler: it splits a line on `;`, `&&`, `||`, `|`, `&` and newlines outside quotes, and gives up
// (not confident) on anything it does not understand: substitutions, heredocs, `eval`, unbalanced quotes, `sh -c`.

import type { CommandAnalysis } from "../types";

const GIVE_UP = [/\$\(/, /`/, /<\(/, />\(/, /<</, /\beval\b/, /\b(?:ba|z|da|k)?sh\s+-\w*c\b/, /^\s*[\w.]*\$/, /\(\(/];

export function analyzeCommand(line: string): CommandAnalysis {
  const parts: string[] = [];
  let cur = "";
  let quote: "'" | '"' | null = null;
  let confident = true;
  const flush = () => {
    const t = cur.trim().split(/\s+/).filter(Boolean).join(" ");
    if (t) parts.push(t);
    cur = "";
  };
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === quote) quote = null;
      else if (c === "\\" && quote === '"') cur += line[++i] ?? "";
      else cur += c;
      continue;
    }
    if (c === "'" || c === '"') quote = c;
    else if (c === "\\") cur += line[++i] ?? "";
    else if (c === ";" || c === "\n" || c === "|" || c === "&") {
      flush();
      if ((c === "&" || c === "|") && line[i + 1] === c) i++;
    } else if (c === "#" && (cur === "" || /\s$/.test(cur))) {
      while (i < line.length && line[i] !== "\n") i++;
      i--;
    } else cur += c;
  }
  flush();
  if (quote !== null || GIVE_UP.some((re) => re.test(line))) confident = false;
  // Leading `VAR=value` words are not commands.
  const commands = parts.map((p) => p.replace(/^(?:[A-Za-z_]\w*=\S*\s+)+/, "")).filter(Boolean);
  return { commands, also: [line.trim()].filter((l) => l && !commands.includes(l)), confident };
}
