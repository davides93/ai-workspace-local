// Editable lists (rules, hooks, workflow steps) need a stable identity per row: the position changes
// when rows are moved or removed, and React must keep each row's inputs and focus with the row.

export interface Keyed<T> {
  uid: number;
  value: T;
}

let seq = 0;

/** Gives every item a stable uid. */
export function keyed<T>(items: readonly T[]): Keyed<T>[] {
  return items.map((value) => ({ uid: ++seq, value }));
}

export function keyedOne<T>(value: T): Keyed<T> {
  return { uid: ++seq, value };
}

export function unkeyed<T>(rows: readonly Keyed<T>[]): T[] {
  return rows.map((r) => r.value);
}

/** Moves the row with `uid` by `delta` positions (clamped). */
export function moveRow<T>(rows: readonly Keyed<T>[], uid: number, delta: number): Keyed<T>[] {
  const from = rows.findIndex((r) => r.uid === uid);
  if (from < 0) return [...rows];
  const to = Math.min(rows.length - 1, Math.max(0, from + delta));
  if (to === from) return [...rows];
  const next = [...rows];
  const [row] = next.splice(from, 1);
  next.splice(to, 0, row);
  return next;
}

/** Applies a same-length transformation of the plain values to the rows, keeping every uid. */
export function mapKeyed<T>(rows: readonly Keyed<T>[], fn: (values: T[]) => T[]): Keyed<T>[] {
  const out = fn(rows.map((r) => r.value));
  return rows.map((r, i) => ({ uid: r.uid, value: out[i] }));
}
