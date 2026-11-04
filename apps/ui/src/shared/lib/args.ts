// Tool arguments arrive as a JSON string (tool cards) or as a parsed value (confirmation requests).
// These helpers turn either into something readable.

/** Parses a JSON string of arguments; the raw text when it is not valid JSON (a call still streaming). */
export function parseArgs(raw: string): unknown {
  if (!raw.trim()) return {};
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return raw;
  }
}

/** Indented JSON for a value; a string that holds JSON is re-indented, any other string stays as it is. */
export function prettyJson(value: unknown): string {
  const v = typeof value === "string" ? parseArgs(value) : value;
  if (typeof v === "string") return v;
  try {
    return JSON.stringify(v, null, 2) ?? "";
  } catch {
    return String(v);
  }
}

const oneLine = (s: string) => s.replace(/\s*\n\s*/g, " ⏎ ").replace(/\s+/g, " ").trim();
const clip = (s: string, max: number) => (s.length > max ? `${s.slice(0, Math.max(0, max - 1))}…` : s);

/** `key=value  key=value` on one line (strings as they are, other values as JSON), cut at `max` characters. */
export function summarizeArgs(args: unknown, max = 160): string {
  const v = typeof args === "string" ? parseArgs(args) : args;
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return clip(oneLine(v), max);
  if (typeof v !== "object" || Array.isArray(v)) return clip(oneLine(JSON.stringify(v) ?? ""), max);
  const parts = Object.entries(v as Record<string, unknown>).map(([k, val]) => `${k}=${typeof val === "string" ? oneLine(val) : JSON.stringify(val)}`);
  return clip(parts.join("  "), max);
}
