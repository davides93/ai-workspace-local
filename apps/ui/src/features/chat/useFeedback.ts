import { useCallback, useMemo, useRef } from "react";
import type { Feedback } from "../../shared/api/types";
import { useDeleteFeedback, useSessionFeedback, useSetFeedback } from "./api";
import { finalAnswerIds, ratingsById } from "./model/feedback";
import type { Item } from "./model/items";

export interface FeedbackController {
  /** Ratings of the open session by message id. */
  ratings: Map<string, Feedback>;
  /** Replies that can be rated. */
  rateable: Set<string>;
  /** A rating is on its way for this message. */
  pendingId: string | null;
  onRate: (messageId: string, rating: 1 | -1, note?: string) => void;
  onClear: (messageId: string) => void;
}

/** Thumbs up/down on the final answers of the open conversation. The callbacks keep their identity (rows are memoized). */
export function useFeedback(sessionId: string | null, items: Item[], running: boolean): FeedbackController {
  const list = useSessionFeedback(sessionId);
  const set = useSetFeedback(sessionId);
  const del = useDeleteFeedback(sessionId);
  const ratings = useMemo(() => ratingsById(list.data), [list.data]);
  const rateable = useMemo(() => finalAnswerIds(items, running), [items, running]);
  const latest = useRef({ set, del });
  latest.current = { set, del };
  const onRate = useCallback((messageId: string, rating: 1 | -1, note?: string) => latest.current.set.mutate({ messageId, rating, note }), []);
  const onClear = useCallback((messageId: string) => latest.current.del.mutate(messageId), []);
  const pendingId = set.isPending ? set.variables?.messageId ?? null : del.isPending ? del.variables ?? null : null;
  return { ratings, rateable, pendingId, onRate, onClear };
}
