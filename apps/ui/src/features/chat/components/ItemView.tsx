import { memo } from "react";
import { Icon, Led, Markdown } from "../../../shared/ui";
import { ApprovalCard } from "../../approvals/components/ApprovalCard";
import { A2uiSurface, type A2uiAction } from "../a2ui";
import type { ApprovalRequest, Feedback } from "../../../shared/api/types";
import type { Item } from "../model/items";
import { describeRunError, errorDetail } from "../model/runNotice";
import { MessageActions } from "./MessageActions";
import { RunNote } from "./RunNote";
import { SubagentCard } from "./SubagentCard";
import { ToolCard } from "./ToolCard";

interface Props {
  it: Item;
  onRespond: (r: ApprovalRequest, d: "allow" | "deny", remember?: boolean) => void;
  onAction: (a: A2uiAction) => void;
  /** A run is going on: surfaces are read-only until it ends. */
  busy: boolean;
  /** Id of the confirmation whose answer is on its way. */
  respondingId?: string | null;
  /** Thumbs up/down: whether this row is a final answer, its rating and the actions on it. */
  rateable?: boolean;
  rating?: Feedback;
  ratePending?: boolean;
  onRate?: (messageId: string, rating: 1 | -1, note?: string) => void;
  onClear?: (messageId: string) => void;
}

const noop = () => {};

const contextText = (ctx: Record<string, unknown>) => Object.entries(ctx).map(([k, v]) => `${k}=${typeof v === "string" ? v : JSON.stringify(v)}`).join("  ");

/**
 * One row of the conversation. Memoized: the reducer keeps the identity of rows that did not
 * change, so while one message streams every other row is skipped.
 */
export const ItemView = memo(function ItemView({ it, onRespond, onAction, busy, respondingId, rateable, rating, ratePending, onRate, onClear }: Props) {
  switch (it.kind) {
    case "surface":
      return <div className="ws-msg ws-assistant"><A2uiSurface surface={it.surface} onAction={onAction} disabled={busy} /></div>;
    case "action":
      return (
        <div className="ws-msg ws-user">
          <div className="ws-action" title="Azione inviata dall'interfaccia">
            <Icon name="play" size={13} /> <b>{it.name}</b>
            {Object.keys(it.context).length > 0 && <span className="mono">{contextText(it.context)}</span>}
          </div>
        </div>
      );
    case "user":
      return <div className="ws-msg ws-user"><div className="ws-bubble-user">{it.text}</div></div>;
    case "text":
      return (
        <div className="ws-msg ws-assistant">
          <div className="ws-bubble"><Markdown text={it.text} streaming={it.streaming} /></div>
          {!it.streaming && it.text && (
            <MessageActions
              messageId={it.id} text={it.text} rateable={!!rateable && !!onRate && !!onClear} rating={rating} pending={ratePending}
              onRate={onRate ?? noop} onClear={onClear ?? noop}
            />
          )}
        </div>
      );
    case "reasoning":
      return (
        <div className="ws-msg ws-assistant">
          <details className="ws-reasoning" open={it.streaming || undefined}>
            <summary>{it.streaming && <Led tone="run" small />}{it.streaming ? "Sta ragionando…" : "Ragionamento"}</summary>
            <div className="ws-reasoning-text">{it.text}</div>
          </details>
        </div>
      );
    case "tool":
      // The surface itself is shown; keep the card only when the call did not produce one.
      if (it.name === "render_ui" && it.status === "done") return null;
      return <div className="ws-msg ws-assistant"><ToolCard tool={it} /></div>;
    case "approval":
      return (
        <div className="ws-msg ws-assistant">
          <ApprovalCard
            request={it.req}
            resolved={it.resolved}
            allowRemember
            pending={respondingId === it.req.id}
            onRespond={(decision, remember) => onRespond(it.req, decision, remember)}
          />
        </div>
      );
    case "subagent":
      return (
        <div className="ws-msg ws-assistant">
          <SubagentCard it={it} renderItem={(row) => <ItemView it={row} onRespond={onRespond} onAction={onAction} busy respondingId={null} />} />
        </div>
      );
    case "note":
      return <div className="ws-msg ws-assistant"><div className="ws-loopnote"><Icon name="refresh" size={14} /> {it.text}</div></div>;
    case "error":
      return (
        <div className="ws-msg ws-assistant">
          <RunNote live={!it.stored} info={describeRunError(it.code)} message={errorDetail(it.code, it.message)} />
        </div>
      );
  }
});
