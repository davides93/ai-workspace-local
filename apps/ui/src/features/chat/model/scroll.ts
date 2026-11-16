import type { Item } from "./items";

/** The replies in this many rows at the end of a conversation, as it was when it opened, are never skipped (`.ws-bubble[data-cold]`). */
export const COLD_TAIL = 40;

/**
 * Rows before this index are «cold»: far from the end of the conversation **as it was when it opened**. The boundary is fixed at that
 * moment, not moved as rows arrive: a row that turned cold while the reader was at the end would change height (its real size becomes
 * the estimate of the skipped box) above the screen, the browser would move the scroll position to compensate, and the view would stop
 * following the end. The rows that arrive in the session stay as they are until the conversation is opened again.
 */
export const coldBefore = (loadedCount: number, tail = COLD_TAIL): number => Math.max(0, loadedCount - tail);

export interface ScrollMetrics {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

/** Pixels hidden below the visible part of a scroll container. */
export const distanceFromBottom = (m: ScrollMetrics): number => Math.max(0, m.scrollHeight - m.scrollTop - m.clientHeight);

/** Is the reader at (or within `threshold` px of) the bottom? Only then does new content pull the view along. */
export const isNearBottom = (m: ScrollMetrics, threshold = 64): boolean => distanceFromBottom(m) <= threshold;

/**
 * What the end of a conversation looks like, as a string that changes when something new arrives there: a row is added, the last
 * one grows (streaming text, a tool result, an answered confirmation), a note or a proposal shows up under it. The "Nuovi messaggi"
 * pill follows this and not the height of the list: with long conversations the rows far from the end are laid out only when
 * they come near the screen, so the height changes while the reader scrolls through the past, and nothing has arrived.
 */
export function tailSignature(items: readonly Item[], below: { notice?: boolean; footer?: boolean } = {}): string {
  const last = items[items.length - 1];
  let tail = "";
  if (last) {
    switch (last.kind) {
      case "text":
      case "reasoning":
        tail = `${last.text.length}${last.streaming ? "s" : ""}`;
        break;
      case "tool":
        tail = `${last.status}${last.result?.length ?? 0}${last.images?.length ?? 0}`;
        break;
      case "approval":
        tail = last.resolved ? "r" : "w";
        break;
      case "question":
      case "secret":
        tail = last.outcome ?? "w";
        break;
      case "subagent":
        tail = `${last.result?.length ?? 0}${last.interrupted ? "i" : ""}`;
        break;
      case "surface":
        // `rev` is bumped when the server sends a new data model; the number of components changes when it adds or removes some.
        tail = `${last.surface.rev}.${Object.keys(last.surface.components).length}${last.surface.deleted ? "d" : ""}`;
        break;
      case "note":
      case "warning":
        tail = String(last.text.length);
        break;
      case "error":
        tail = String(last.message.length);
        break;
      default:
        tail = "";
    }
  }
  return `${items.length}|${last?.kind ?? ""}|${tail}|${below.notice ? 1 : 0}${below.footer ? 1 : 0}`;
}
