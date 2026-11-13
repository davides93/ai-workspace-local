import { forwardRef, useImperativeHandle, useRef, type ReactNode } from "react";
import { ErrorState, Icon, SkeletonLines } from "../../../shared/ui";
import type { A2uiAction } from "../a2ui";
import type { ApprovalRequest } from "../../../shared/api/types";
import { itemKey, type Item } from "../model/items";
import type { RunNoticeInfo } from "../model/runNotice";
import type { FeedbackController } from "../useFeedback";
import { ItemView } from "./ItemView";
import { RunNote } from "./RunNote";
import { useStickToBottom } from "./useStickToBottom";

import { t, t as tr } from "../../../shared/i18n";
export interface ConversationLogHandle {
  scrollToBottom: () => void;
}

interface Props {
  items: Item[];
  /** `loading` while the history is fetched, `error` when that failed. */
  state: { status: "loading" | "ready" | "error"; error?: unknown; title?: string };
  onRetry: () => void;
  busy: boolean;
  respondingId?: string | null;
  onRespond: (r: ApprovalRequest, d: "allow" | "deny", remember?: boolean) => void;
  onAction: (a: A2uiAction) => void;
  /** Note under a history that ends with a stopped or failed run. */
  notice: RunNoticeInfo | null;
  /** Shown when the conversation is empty. */
  empty: ReactNode;
  /** A different conversation: the view starts at its end. */
  sessionKey: string | null;
  /** Thumbs up/down on the final answers. */
  feedback?: FeedbackController;
  /** Something that belongs to the conversation after its last row (a proposal waiting for an answer). */
  footer?: ReactNode;
}

/**
 * The scrolling list of rows. It follows the end of the conversation only while the reader is
 * there; otherwise a "Nuovi messaggi" pill appears. Announces nothing token by token: the live
 * region of the view tells when a reply starts and ends.
 */
export const ConversationLog = forwardRef<ConversationLogHandle, Props>(function ConversationLog(
  { items, state, onRetry, busy, respondingId, onRespond, onAction, notice, empty, sessionKey, feedback, footer },
  ref,
) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const { unseen, scrollToBottom } = useStickToBottom(scrollRef, contentRef, sessionKey);
  useImperativeHandle(ref, () => ({ scrollToBottom }), [scrollToBottom]);

  return (
    <div className="ws-log-wrap">
      <div className="ws-log" ref={scrollRef} role="log" aria-label={t("chat.conversazione")} aria-live="off" tabIndex={0}>
        <div className="ws-log-inner" ref={contentRef}>
          {state.status === "loading" && <SkeletonLines lines={5} />}
          {state.status === "error" && <ErrorState title={state.title ?? t("chat.impossibile_caricare_la_conversazione")} error={state.error} onRetry={onRetry} />}
          {state.status === "ready" && items.length === 0 && empty}
          {state.status === "ready" && items.map((it, i) => (
            <ItemView
              key={itemKey(it, i)} it={it} sessionId={sessionKey ?? undefined} onRespond={onRespond} onAction={onAction} busy={busy} respondingId={respondingId}
              {...(it.kind === "text" && feedback
                ? {
                    rateable: feedback.rateable.has(it.id), rating: feedback.ratings.get(it.id), ratePending: feedback.pendingId === it.id,
                    onRate: feedback.onRate, onClear: feedback.onClear,
                  }
                : {})}
            />
          ))}
          {state.status === "ready" && notice && <RunNote info={notice} />}
          {state.status === "ready" && footer}
        </div>
      </div>
      {unseen && (
        <button type="button" className="ws-newmsg" onClick={scrollToBottom}>
          <Icon name="down" size={14} /> {" "}{tr("chat.nuovi_messaggi")}</button>
      )}
    </div>
  );
});
