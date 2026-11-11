import { useId, useState } from "react";
import { Link } from "../../../app/router";
import { relTime } from "../../../shared/lib/format";
import { Button, Field, Pill, Textarea } from "../../../shared/ui";
import type { Proposal } from "../../../shared/api/types";
import { editedInstruction, STATUS_LABEL, STATUS_TONE } from "../model/proposals";

/** One thing the assistant noticed: what, and what it would do. Nothing happens until Approva. */
export function ProposalCard({ proposal, busy, onApprove, onDismiss }: {
  proposal: Proposal;
  busy: boolean;
  onApprove: (instruction?: string) => void;
  onDismiss: () => void;
}) {
  const id = useId();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(proposal.instruction);
  const cancel = () => {
    setText(proposal.instruction);
    setEditing(false);
  };
  return (
    <article className="ib-card" aria-labelledby={`${id}-t`}>
      <header className="ib-head">
        <b id={`${id}-t`} className="ib-title">{proposal.title}</b>
        <time className="faint text-xs" dateTime={proposal.created_at}>{relTime(proposal.created_at)}</time>
      </header>
      <p className="muted text-sm">{proposal.noticed}</p>
      {editing ? (
        <Field label="Cosa faccio" htmlFor={`${id}-i`}>
          <Textarea id={`${id}-i`} rows={4} value={text} onChange={(e) => setText(e.target.value)} />
        </Field>
      ) : (
        <p className="ib-plan text-sm"><span className="faint">Cosa farei: </span>{proposal.instruction}</p>
      )}
      <div className="ib-actions">
        <Button size="sm" variant="primary" icon="check" disabled={busy || !text.trim()} onClick={() => onApprove(editedInstruction(proposal, text))}>Approva</Button>
        {editing ? (
          <Button size="sm" variant="ghost" disabled={busy} onClick={cancel}>Annulla le modifiche</Button>
        ) : (
          <Button size="sm" icon="edit" disabled={busy} onClick={() => setEditing(true)}>Modifica</Button>
        )}
        <Button size="sm" variant="ghost" icon="close" disabled={busy} onClick={onDismiss}>Scarta</Button>
      </div>
    </article>
  );
}

/** A line of the "Fatte" list. */
export function DoneRow({ proposal }: { proposal: Proposal }) {
  return (
    <li className="ib-done">
      <Pill tone={STATUS_TONE[proposal.status]}>{STATUS_LABEL[proposal.status]}</Pill>
      <span className="ib-done-title">{proposal.title}</span>
      {proposal.session_id && <Link to={`/chat/${encodeURIComponent(proposal.session_id)}`} className="text-sm">Apri la chat</Link>}
      <time className="faint text-xs" dateTime={proposal.decided_at ?? proposal.created_at}>{relTime(proposal.decided_at ?? proposal.created_at)}</time>
    </li>
  );
}
