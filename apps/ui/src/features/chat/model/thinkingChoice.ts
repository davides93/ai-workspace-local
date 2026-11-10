import { isThinkingMode } from "../../../shared/lib/thinking";
import type { ThinkingMode } from "../../../shared/api/types";

// The "Ragionamento" chosen in each conversation. It is a convenience of this device, not part of the
// session on the backend: it lives in memory and in the browser storage (which may be unavailable).
const chosen = new Map<string, ThinkingMode>();
const KEY = (sessionId: string) => `aiw.chat.thinking.${sessionId}`;

export function getThinkingChoice(sessionId: string | null): ThinkingMode | undefined {
  if (!sessionId) return undefined;
  const mem = chosen.get(sessionId);
  if (mem) return mem;
  try {
    const v = localStorage.getItem(KEY(sessionId));
    if (isThinkingMode(v)) {
      chosen.set(sessionId, v);
      return v;
    }
  } catch {
    /* storage unavailable: the choice lasts until the page is reloaded */
  }
  return undefined;
}

export function setThinkingChoice(sessionId: string | null, mode: ThinkingMode): void {
  if (!sessionId) return;
  chosen.set(sessionId, mode);
  try {
    localStorage.setItem(KEY(sessionId), mode);
  } catch {
    /* see above */
  }
}

/** A deleted conversation takes its choice with it. */
export function forgetThinkingChoice(sessionId: string): void {
  chosen.delete(sessionId);
  try {
    localStorage.removeItem(KEY(sessionId));
  } catch {
    /* see above */
  }
}
