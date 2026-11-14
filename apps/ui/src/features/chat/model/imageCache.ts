// Screenshots of tool calls (`aiw.image` events). The stored messages of a session carry only text, so
// the chat keeps the images it has seen (live events, or the list the backend replays from the stored
// run events when a session is opened: `GET /sessions/{id}/images`) within a memory budget. A session
// that falls out of the budget is loaded from the backend again when it is reopened.
import type { ToolImage } from "./items";
import { parseImageEvent } from "./reduce";

const MAX_CHARS = 24_000_000;

const sessions = new Map<string, Map<string, ToolImage[]>>();
const sizes = new Map<string, number>();
let total = 0;

export function rememberImage(sessionId: string, toolCallId: string, image: ToolImage): void {
  const forSession = sessions.get(sessionId) ?? new Map<string, ToolImage[]>();
  const list = forSession.get(toolCallId) ?? [];
  if (list.some((x) => x.data === image.data)) return;
  forSession.set(toolCallId, [...list, image]);
  // Re-insert so the session counts as the most recently used.
  sessions.delete(sessionId);
  sessions.set(sessionId, forSession);
  sizes.set(sessionId, (sizes.get(sessionId) ?? 0) + image.data.length);
  total += image.data.length;
  while (total > MAX_CHARS && sessions.size > 1) {
    const oldest = sessions.keys().next().value as string;
    if (oldest === sessionId) break;
    forgetSessionImages(oldest);
  }
}

/**
 * Remembers the list the backend sent for a session (entries shaped like the `aiw.image` event value, plus `run_id`).
 * Anything that is not a well-formed image is skipped; images already remembered are not added twice.
 */
export function rememberSessionImages(sessionId: string, list: unknown): void {
  if (!Array.isArray(list)) return;
  for (const entry of list) {
    const ev = parseImageEvent(entry);
    if (ev) rememberImage(sessionId, ev.toolCallId, ev.image);
  }
}

/** Lookup of the images remembered for a session, by tool call id. */
export const imagesFor = (sessionId: string) => (toolCallId: string): ToolImage[] | undefined => sessions.get(sessionId)?.get(toolCallId);

export function forgetSessionImages(sessionId: string): void {
  total -= sizes.get(sessionId) ?? 0;
  sizes.delete(sessionId);
  sessions.delete(sessionId);
}

export const cachedImageChars = (): number => total;
