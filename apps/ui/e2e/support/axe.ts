import type { Result } from "axe-core";

/**
 * Rules switched off for every scan, each with the reason. Keep this list empty unless a violation
 * cannot reasonably be fixed; every entry must be named in docs/STATUS.md.
 */
export const DISABLED_RULES: { id: string; reason: string }[] = [];

export const isBlocking = (v: Result): boolean => v.impact === "critical" || v.impact === "serious";

/** A readable summary: rule, impact, help, and the first targets of every violation. */
export function formatViolations(violations: Result[]): string {
  if (violations.length === 0) return "no violations";
  return violations
    .map((v) => {
      const nodes = v.nodes.slice(0, 5).map((n) => `    - ${n.target.join(" ")}\n      ${n.failureSummary?.replace(/\n/g, "\n      ") ?? ""}`).join("\n");
      return `[${v.impact}] ${v.id}: ${v.help} (${v.nodes.length} element${v.nodes.length === 1 ? "" : "s"})\n${nodes}`;
    })
    .join("\n");
}
