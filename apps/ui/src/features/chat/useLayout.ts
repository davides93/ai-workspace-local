import { useCallback, useEffect, useState } from "react";
import { useMediaQuery } from "../../shared/lib/useMediaQuery";
import { getDraft, setDraft } from "./model/drafts";

/** Below this width the sessions column is an off-canvas drawer. */
export const COMPACT_SESSIONS_QUERY = "(max-width: 1000px)";

export type InspectorMode = "inline" | "drawer" | "sheet";

/**
 * Where the run panel lives: a column next to the conversation on wide screens, a side drawer on
 * tablets (the conversation would be left too narrow), a bottom sheet on phones.
 */
export function useInspectorMode(): InspectorMode {
  const phone = useMediaQuery("(max-width: 760px)");
  const wide = useMediaQuery("(min-width: 1320px)");
  return phone ? "sheet" : wide ? "inline" : "drawer";
}

const KEY = "aiw.chat.inspector";

function readInline(): boolean {
  try {
    return localStorage.getItem(KEY) !== "0";
  } catch {
    return true;
  }
}

/** Open state of the run panel: remembered for the inline column, always closed to begin with as an overlay. */
export function useInspectorOpen(mode: InspectorMode) {
  const [inlineOpen, setInlineOpen] = useState(readInline);
  const [overlayOpen, setOverlayOpen] = useState(false);
  // Resizing across a breakpoint must not leave an overlay open.
  useEffect(() => setOverlayOpen(false), [mode]);
  const open = mode === "inline" ? inlineOpen : overlayOpen;
  const set = useCallback(
    (next: boolean) => {
      if (mode === "inline") {
        setInlineOpen(next);
        try {
          localStorage.setItem(KEY, next ? "1" : "0");
        } catch {
          /* storage unavailable: the choice lasts until the page is closed */
        }
      } else setOverlayOpen(next);
    },
    [mode],
  );
  return { open, show: () => set(true), close: () => set(false), toggle: () => set(!open) };
}

/** The unsent message of a session: each session keeps its own text while the user moves around. */
export function useDraft(sessionId: string | null) {
  const [input, setInputState] = useState(() => getDraft(sessionId));
  useEffect(() => setInputState(getDraft(sessionId)), [sessionId]);
  const setInput = useCallback(
    (v: string) => {
      setInputState(v);
      setDraft(sessionId, v);
    },
    [sessionId],
  );
  return [input, setInput] as const;
}
