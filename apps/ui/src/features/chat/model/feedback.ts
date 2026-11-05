import type { Feedback } from "../../../shared/api/types";
import type { Item } from "./items";

/**
 * Ids of the replies that can be rated: the final answer of each turn, i.e. the last text the assistant wrote
 * before the next user message with nothing (a tool call, a confirmation, a sub-agent, a self-check) after it.
 * The turn that is still going on has no final answer yet.
 */
export function finalAnswerIds(items: Item[], running: boolean): Set<string> {
  const out = new Set<string>();
  let candidate: string | null = null;
  const commit = () => {
    if (candidate) out.add(candidate);
    candidate = null;
  };
  for (const it of items) {
    switch (it.kind) {
      case "user":
      case "action":
        commit();
        break;
      case "text":
        candidate = !it.streaming && it.text.trim() ? it.id : null;
        break;
      case "tool":
      case "approval":
      case "subagent":
      case "note":
      case "surface":
        candidate = null;
        break;
      default:
        break; // reasoning and errors do not change which text was the last
    }
  }
  if (!running) commit();
  return out;
}

export type RatingChange = { kind: "set"; rating: 1 | -1 } | { kind: "clear" };

/** What a click on a thumb does: the same thumb again takes the rating back, the other one changes it. */
export function ratingChange(current: Feedback | undefined, clicked: 1 | -1): RatingChange {
  return current?.rating === clicked ? { kind: "clear" } : { kind: "set", rating: clicked };
}

/** The ratings of a session by message id. */
export function ratingsById(list: Feedback[] | undefined): Map<string, Feedback> {
  return new Map((list ?? []).map((f) => [f.message_id, f]));
}

/** Replaces (or adds) the rating of a message in a list of ratings. */
export function withRating(list: Feedback[] | undefined, f: Feedback): Feedback[] {
  return [...(list ?? []).filter((x) => x.message_id !== f.message_id), f];
}

export const withoutRating = (list: Feedback[] | undefined, messageId: string): Feedback[] =>
  (list ?? []).filter((x) => x.message_id !== messageId);
