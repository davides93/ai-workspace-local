// Editable key/value rows (environment variables, HTTP headers) and their conversion to the
// `Record<string, string>` the API stores.

export interface KvRow {
  /** Stable identity of the row while editing (keeps focus when the order or content changes). */
  id: string;
  key: string;
  value: string;
}

let seq = 0;

export function emptyRow(key = "", value = ""): KvRow {
  return { id: `kv${++seq}`, key, value };
}

/** One row per entry, in the order the server sent them. Non-string values are shown as JSON. */
export function recordToRows(rec: Record<string, unknown> | null | undefined): KvRow[] {
  return Object.entries(rec ?? {}).map(([k, v]) => emptyRow(k, typeof v === "string" ? v : JSON.stringify(v)));
}

/** Rows without a name are dropped; names are trimmed; a repeated name keeps the last value. */
export function rowsToRecord(rows: KvRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const r of rows) {
    const k = r.key.trim();
    if (k) out[k] = r.value;
  }
  return out;
}

/** Names used by more than one row (optionally without regard to case, as HTTP headers are). */
export function duplicateKeys(rows: KvRow[], caseInsensitive = false): string[] {
  const seen = new Set<string>();
  const dup = new Set<string>();
  for (const r of rows) {
    const k = caseInsensitive ? r.key.trim().toLowerCase() : r.key.trim();
    if (!k) continue;
    if (seen.has(k)) dup.add(k);
    seen.add(k);
  }
  return [...dup];
}
