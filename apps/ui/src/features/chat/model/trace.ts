import type { AgEvent } from "../../../shared/api/types";
import type { Item } from "./items";
import { reduce } from "./reduce";

/**
 * The rows of a sub-agent's trace, from the events of its run. The confirmations are left out (they are
 * mirrored to the parent run and answered in the conversation, and the trace is read-only), and so is the
 * run's own error row: the card already says how the sub-agent ended.
 */
export function traceItems(events: AgEvent[]): Item[] {
  return events.reduce<Item[]>((items, e) => reduce(items, e), []).filter((i) => i.kind !== "approval" && i.kind !== "error");
}
