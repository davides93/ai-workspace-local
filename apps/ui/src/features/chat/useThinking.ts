import { useCallback, useEffect, useState } from "react";
import type { ThinkingMode } from "../../shared/api/types";
import { effectiveThinking } from "../../shared/lib/thinking";
import { getThinkingChoice, setThinkingChoice } from "./model/thinkingChoice";

/**
 * The "Ragionamento" of the open conversation: what was chosen here (remembered per session), else
 * what the profile says, else automatic; and a setter.
 */
export function useThinking(sessionId: string | null, profileMode: ThinkingMode | undefined): [ThinkingMode, (mode: ThinkingMode) => void] {
  const [chosen, setChosen] = useState<ThinkingMode | undefined>(() => getThinkingChoice(sessionId));
  useEffect(() => setChosen(getThinkingChoice(sessionId)), [sessionId]);
  const set = useCallback(
    (mode: ThinkingMode) => {
      setThinkingChoice(sessionId, mode);
      setChosen(mode);
    },
    [sessionId],
  );
  return [effectiveThinking(chosen, profileMode), set];
}
