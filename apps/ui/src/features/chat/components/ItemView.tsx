import { memo, useState } from "react";
import { Button, Icon, Led, Markdown, Pill } from "../../../shared/ui";
import { RISK_LABEL, RISK_TONE } from "../../../shared/lib/risk";
import { A2uiSurface, type A2uiAction } from "../a2ui";
import type { ApprovalRequest } from "../../../shared/api/types";
import type { Item } from "../model/items";

function prettyArgs(args: string): string {
  try {
    return JSON.stringify(JSON.parse(args), null, 2);
  } catch {
    return args;
  }
}

function argSummary(args: string): string {
  try {
    const o = JSON.parse(args) as Record<string, unknown>;
    return Object.entries(o).map(([k, v]) => `${k}=${typeof v === "string" ? v : JSON.stringify(v)}`).join("  ");
  } catch {
    return args;
  }
}

const toolIcon = (name: string) =>
  name.startsWith("browser") || name === "web_fetch" ? "globe" : name === "shell" ? "terminal" : name.startsWith("computer") ? "mouse" : name.startsWith("mcp__") ? "link" : "file";

interface Props {
  it: Item;
  onRespond: (r: ApprovalRequest, d: "allow" | "deny", remember?: boolean) => void;
  onAction: (a: A2uiAction) => void;
  busy: boolean;
}

/** One row of the conversation. Memoized: rows that did not change are not re-rendered while another one streams. */
export const ItemView = memo(function ItemView({ it, onRespond, onAction, busy }: Props) {
  const [open, setOpen] = useState(false);
  switch (it.kind) {
    case "surface":
      return <div className="msg assistant"><A2uiSurface surface={it.surface} onAction={onAction} disabled={busy} /></div>;
    case "action":
      return (
        <div className="msg user">
          <div className="action-chip" title="Azione inviata dall'interfaccia">
            <Icon name="play" size={13} /> <b>{it.name}</b>
            {Object.keys(it.context).length > 0 && (
              <span className="mono">{Object.entries(it.context).map(([k, v]) => `${k}=${typeof v === "string" ? v : JSON.stringify(v)}`).join("  ")}</span>
            )}
          </div>
        </div>
      );
    case "user":
      return <div className="msg user"><div className="bubble">{it.text}</div></div>;
    case "text":
      return (
        <div className="msg assistant">
          <div className="bubble"><Markdown text={it.text + (it.streaming ? " ▍" : "")} /></div>
        </div>
      );
    case "reasoning":
      return (
        <div className="msg assistant">
          <details className="reasoning" open={it.streaming}>
            <summary>{it.streaming ? "Sta ragionando…" : "Ragionamento"}</summary>
            {it.text}
          </details>
        </div>
      );
    case "tool":
      // The surface itself is shown; keep the card only when the call failed.
      if (it.name === "render_ui" && !(it.result ?? "").startsWith("ERRORE")) return null;
      return (
        <div className="msg assistant">
          <div className="toolcard">
            <button type="button" className="toolcard-head" onClick={() => setOpen((v) => !v)} aria-expanded={open}>
              <Icon name={toolIcon(it.name)} size={15} />
              <span className="name">{it.name}</span>
              <span className="args">{argSummary(it.args)}</span>
              {it.status === "running" ? <Led tone="run" /> : it.status === "denied" ? <Pill tone="crit">negato</Pill> : <Icon name="check" size={15} />}
            </button>
            {open && <pre>{prettyArgs(it.args)}</pre>}
            {it.result !== undefined && <pre>{it.result}</pre>}
            {it.images?.map((img, i) => (
              <img key={i} className="toolcard-image" src={`data:${img.mime};base64,${img.data}`} alt={`Schermata prodotta da ${it.name}`} />
            ))}
          </div>
        </div>
      );
    case "approval":
      return (
        <div className="msg assistant">
          <div className="approval" role="alert">
            <div className="row">
              <Icon name="shield" />
              <b>Serve la tua conferma</b>
              <Pill tone={RISK_TONE[it.req.risk]}>rischio {RISK_LABEL[it.req.risk]}</Pill>
            </div>
            <span>{it.req.reason}</span>
            <pre>{it.req.tool} {JSON.stringify(it.req.args, null, 2)}</pre>
            {it.resolved === undefined ? (
              <div className="row">
                <Button variant="primary" icon="check" onClick={() => onRespond(it.req, "allow")}>Consenti</Button>
                <Button onClick={() => onRespond(it.req, "allow", true)}>Consenti sempre per questa sessione</Button>
                <Button variant="danger" onClick={() => onRespond(it.req, "deny")}>Nega</Button>
              </div>
            ) : (
              <Pill tone={it.resolved ? "ok" : "crit"}>{it.resolved ? "Consentito" : "Negato"}</Pill>
            )}
          </div>
        </div>
      );
    case "subagent":
      return (
        <div className="msg assistant">
          <div className="subagent">
            <Icon name="agent" size={15} />
            <span>Subagente <b>{it.name}</b></span>
            {it.result ? <span className="muted">→ {it.result}</span> : <Led tone="run" />}
          </div>
        </div>
      );
    case "note":
      return <div className="msg assistant"><div className="loop-note"><Icon name="refresh" size={14} /> {it.text}</div></div>;
    case "error":
      return <div className="msg assistant"><Pill tone="crit">{it.message}</Pill></div>;
  }
});
