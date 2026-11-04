// Editable key/value rows (environment variables, HTTP headers) and their conversion to the
// `Record<string, string>` the API stores.

export interface KvRow {
  /** Stable identity of the row while editing (keeps focus when the order or content changes). */
  id: string;
  key: string;
  value: string;
  /**
   * The server holds a secret under this name and sent it as the mask: while the row keeps that name and that
   * value, saving leaves the stored secret alone. A mask under another name has nothing behind it.
   */
  kept?: string;
}

let seq = 0;

export function emptyRow(key = "", value = ""): KvRow {
  return { id: `kv${++seq}`, key, value };
}

/**
 * One row per entry, in the order the server sent them. Non-string values are shown as JSON. With `mask`, the
 * entries the server sent as that mask are marked as kept (see `KvRow.kept`).
 */
export function recordToRows(rec: Record<string, unknown> | null | undefined, mask?: string): KvRow[] {
  return Object.entries(rec ?? {}).map(([k, v]) => {
    const row = emptyRow(k, typeof v === "string" ? v : JSON.stringify(v));
    return mask !== undefined && v === mask ? { ...row, kept: k } : row;
  });
}

/** The row still stands for the secret the server keeps: same name, value untouched (still the mask). */
export const isKeptRow = (r: KvRow, mask: string): boolean => r.value === mask && r.kept !== undefined && r.kept === r.key.trim();

/**
 * The row after its name was edited. A kept secret does not travel to another name (the backend looks it up by
 * name), so renaming such a row empties its value: the user writes the new one, or goes back to the old name and
 * presses "Mantieni".
 */
export function renameRow(r: KvRow, key: string, mask?: string): KvRow {
  const leavesSecret = mask !== undefined && isKeptRow(r, mask) && key.trim() !== r.kept;
  return { ...r, key, ...(leavesSecret ? { value: "" } : {}) };
}

/** Names of the rows whose value is the mask but that stand for no stored secret (renamed, or typed by hand). */
export function strayMasks(rows: KvRow[], mask: string): string[] {
  return rows.filter((r) => r.key.trim() !== "" && r.value === mask && !isKeptRow(r, mask)).map((r) => r.key.trim());
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
