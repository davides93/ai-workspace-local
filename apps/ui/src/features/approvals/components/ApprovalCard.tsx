import { useId } from "react";
import { Link } from "../../../app/router";
import { prettyJson, summarizeArgs } from "../../../shared/lib/args";
import { cx } from "../../../shared/lib/cx";
import { RISK_LABEL, RISK_TONE } from "../../../shared/lib/risk";
import { chatPathForRun } from "../../../shared/lib/runIndex";
import { Button, CodeBlock, Icon, Pill } from "../../../shared/ui";
import type { ApprovalRequest } from "../../../shared/api/types";
import "../approvals.css";

export interface ApprovalCardProps {
  request: ApprovalRequest;
  /** The decision already taken (`true` = allowed). While undefined the card offers the buttons. */
  resolved?: boolean;
  /** `compact` is for narrow spaces (inspector, overview): smaller buttons, no run line. */
  variant?: "full" | "compact";
  /** A response to this request is on its way. */
  pending?: boolean;
  onRespond?: (decision: "allow" | "deny", remember?: boolean) => void;
  /** Offers "Consenti per la sessione" (only meaningful inside a chat session). */
  allowRemember?: boolean;
  /** The request the drawer shortcuts act on. */
  selected?: boolean;
  /** Shows the A / N key hints on the buttons. */
  showShortcuts?: boolean;
  onSelect?: () => void;
}

/**
 * THE confirmation card: shown inline in the chat, in the run inspector, on the overview and in the
 * confirmation drawer. Tool, risk, reason, the arguments (one line, full JSON on demand) and the
 * decision buttons, or the decision once it is taken.
 */
export function ApprovalCard({ request, resolved, variant = "full", pending, onRespond, allowRemember, selected, showShortcuts, onSelect }: ApprovalCardProps) {
  const id = useId();
  const decided = resolved !== undefined;
  const compact = variant === "compact";
  const summary = summarizeArgs(request.args, 400);
  const chatPath = chatPathForRun(request.run_id);
  const size = compact ? "sm" : "md";
  return (
    <article
      className={cx("apv", `apv-${variant}`, `apv-risk-${request.risk}`, decided ? "is-decided" : "is-open", onSelect && "is-selectable", selected && "is-selected")}
      aria-labelledby={`${id}-tool`}
      aria-current={selected || undefined}
      onFocusCapture={onSelect}
      onClick={onSelect}
    >
      {!decided && !compact && !onSelect && <span className="apv-eyebrow">Serve la tua conferma</span>}
      <header className="apv-head">
        <Icon name="shield" size={16} />
        <b id={`${id}-tool`} className="apv-tool mono">{request.tool}</b>
        <Pill tone={RISK_TONE[request.risk]}>rischio {RISK_LABEL[request.risk]}</Pill>
        {decided && <Pill tone={resolved ? "ok" : "crit"}><Icon name={resolved ? "check" : "close"} size={12} />{resolved ? "Consentito" : "Negato"}</Pill>}
      </header>
      <p className="apv-reason">{request.reason}</p>
      {summary && (
        <details className="apv-args">
          <summary>
            <code className="apv-summary">{summary}</code>
            <span className="apv-more">Argomenti</span>
          </summary>
          <CodeBlock code={prettyJson(request.args)} title="Argomenti" />
        </details>
      )}
      {!decided && onRespond && (
        <div className="apv-actions">
          <Button variant="primary" size={size} icon="check" disabled={pending} aria-keyshortcuts={showShortcuts ? "A" : undefined} onClick={() => onRespond("allow")}>
            Consenti{showShortcuts && <kbd aria-hidden="true">A</kbd>}
          </Button>
          {allowRemember && (
            <Button size={size} disabled={pending} onClick={() => onRespond("allow", true)} title="Non chiedere più conferma per questo tool finché la sessione resta aperta">
              Consenti per la sessione
            </Button>
          )}
          <Button variant="danger" size={size} disabled={pending} aria-keyshortcuts={showShortcuts ? "N" : undefined} onClick={() => onRespond("deny")}>
            Nega{showShortcuts && <kbd aria-hidden="true">N</kbd>}
          </Button>
        </div>
      )}
      {!compact && (
        <footer className="apv-meta">
          <span>Run <code>{request.run_id}</code></span>
          {chatPath && <Link to={chatPath} className="apv-link">Apri la sessione <Icon name="forward" size={12} /></Link>}
        </footer>
      )}
    </article>
  );
}
