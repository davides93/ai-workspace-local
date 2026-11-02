// Screenshots of tool calls (`aiw.image` events). The stored messages of a session carry only text and
// the backend has no list of a session's runs, so an image would vanish when the user opens another
// session and comes back. The chat keeps what it has seen, within a memory budget.
import type { ToolImage } from "./items";

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

/** Lookup of the images remembered for a session, by tool call id. */
export const imagesFor = (sessionId: string) => (toolCallId: string): ToolImage[] | undefined => sessions.get(sessionId)?.get(toolCallId);

export function forgetSessionImages(sessionId: string): void {
  total -= sizes.get(sessionId) ?? 0;
  sizes.delete(sessionId);
  sessions.delete(sessionId);
}

export const cachedImageChars = (): number => total;
