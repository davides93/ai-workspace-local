import { memo } from "react";
import { Icon, Led, Markdown } from "../../../shared/ui";
import { useStoredUiMode } from "../../../shared/lib/uiMode";
import { ApprovalCard } from "../../approvals/components/ApprovalCard";
import { A2uiSurface, type A2uiAction } from "../a2ui";
import type { ApprovalRequest, Feedback } from "../../../shared/api/types";
import { reasoningLabel, type Item } from "../model/items";
import { attachmentsNotice, describeRunError, errorDetail } from "../model/runNotice";
import { MessageAttachments } from "./Attachments";
import { MessageActions } from "./MessageActions";
import { RunNote } from "./RunNote";
import { SubagentCard } from "./SubagentCard";
import { ToolCard } from "./ToolCard";

import { t } from "../../../shared/i18n";
interface Props {
  it: Item;
  /** The open session: where the attachments of a message are read from. */
  sessionId?: string;
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
export const ItemView = memo(function ItemView({ it, sessionId, onRespond, onAction, busy, respondingId, rateable, rating, ratePending, onRate, onClear }: Props) {
  switch (it.kind) {
    case "surface":
      return <div className="ws-msg ws-assistant"><A2uiSurface surface={it.surface} onAction={onAction} disabled={busy} /></div>;
    case "action":
      return (
        <div className="ws-msg ws-user">
          <div className="ws-action" title={t("chat.azione_inviata_dall_interfaccia")}>
            <Icon name="play" size={13} /> <b>{it.name}</b>
            {Object.keys(it.context).length > 0 && <span className="mono">{contextText(it.context)}</span>}
          </div>
        </div>
      );
    case "user":
      return (
        <div className="ws-msg ws-user">
          {it.attachments && it.attachments.length > 0 && <MessageAttachments items={it.attachments} sessionId={sessionId} />}
          {it.text && <div className="ws-bubble-user">{it.text}</div>}
        </div>
      );
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
            <summary>{it.streaming && <Led tone="run" small />}{reasoningLabel(it)}</summary>
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
    case "model":
      return <div className="ws-msg ws-assistant"><span className="ws-modelchip" title={it.title}><Icon name="chip" size={12} /> {it.label}</span></div>;
    case "route":
      return <RouteRow it={it} />;
    case "note":
      return <div className="ws-msg ws-assistant"><div className="ws-loopnote"><Icon name="refresh" size={14} /> {it.text}</div></div>;
    case "warning":
      return <div className="ws-msg ws-assistant"><RunNote live info={attachmentsNotice()} /></div>;
    case "error":
      return (
        <div className="ws-msg ws-assistant">
          <RunNote live={!it.stored} info={describeRunError(it.code)} message={errorDetail(it.code, it.message)} />
        </div>
      );
  }
});

/** Which runtime the router chose and why: details for Developer mode, nothing in Simple mode. */
function RouteRow({ it }: { it: Extract<Item, { kind: "route" }> }) {
  const developer = useStoredUiMode() === "developer";
  if (!developer) return null;
  return (
    <div className="ws-msg ws-assistant">
      <details className="ws-routeinfo">
        <summary><span className="ws-modelchip"><Icon name="chip" size={12} /> {it.label}</span></summary>
        <ul>{it.lines.map((l, i) => <li key={i}>{l}</li>)}</ul>
      </details>
    </div>
  );
}
