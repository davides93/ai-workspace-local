import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { cx } from "../../../shared/lib/cx";
import { relTime } from "../../../shared/lib/format";
import { Icon, IconButton, Led, type LedTone } from "../../../shared/ui";
import type { Session } from "../../../shared/api/types";
import { RUN_STATUS_LABEL } from "../model/labels";
import { MAX_TITLE, validateTitle } from "../model/sessions";
import { useRenameSession } from "../api";

const ledTone = (s: Session): LedTone => (s.last_status === "failed" ? "crit" : s.last_status === "running" ? "run" : s.last_status === "cancelled" ? "warn" : s.last_status ? "ok" : "off");

/** Inline rename: Enter saves, Esc (or leaving the field) cancels. */
function RenameForm({ session, onDone }: { session: Session; onDone: () => void }) {
  const rename = useRenameSession();
  const [value, setValue] = useState(session.title);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const submit = (e?: FormEvent) => {
    e?.preventDefault();
    const v = validateTitle(value);
    if (!v.ok) return setError(v.error);
    if (v.title === session.title) return onDone();
    rename.mutate({ id: session.id, title: v.title }, { onSuccess: onDone });
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onDone();
    }
  };
  return (
    <form className="ws-rename" onSubmit={submit} onKeyDown={onKey} onBlur={(e) => !e.currentTarget.contains(e.relatedTarget as Node | null) && !rename.isPending && onDone()}>
      <div className="ws-rename-row">
        <input
          ref={input}
          className="input"
          aria-label="Nuovo titolo della sessione"
          aria-invalid={error ? true : undefined}
          value={value}
          maxLength={MAX_TITLE}
          disabled={rename.isPending}
          onChange={(e) => {
            setValue(e.target.value);
            setError(null);
          }}
        />
        <IconButton type="submit" aria-label="Salva il titolo" icon="check" size={16} disabled={rename.isPending} />
        <IconButton aria-label="Annulla la modifica" icon="close" size={16} onClick={onDone} />
      </div>
      {error && <span className="field-error" role="alert">{error}</span>}
    </form>
  );
}

/** One session of the list: open it, rename it (inline) or delete it (the parent asks to confirm). */
export function SessionItem({ session, agentName, active, renaming, onSelect, onRename, onRenamed, onDelete }: {
  session: Session;
  agentName: string;
  active: boolean;
  renaming: boolean;
  onSelect: () => void;
  onRename: () => void;
  onRenamed: () => void;
  onDelete: () => void;
}) {
  const titleId = useId();
  const status = session.last_status ? RUN_STATUS_LABEL[session.last_status] ?? session.last_status : "Nessun run";
  return (
    <li className={cx("ws-session", active && "is-active")}>
      {renaming ? (
        <RenameForm session={session} onDone={onRenamed} />
      ) : (
        <>
          <button type="button" className="ws-session-main" aria-current={active ? "true" : undefined} onClick={onSelect}>
            <span className="ws-session-title" id={titleId}>{session.title}</span>
            <span className="ws-session-meta">
              <Led tone={ledTone(session)} small />
              <span className="sr-only">{status}. </span>
              <span className="ws-session-agent">{agentName}</span>
              <span aria-hidden="true">·</span>
              <time dateTime={session.updated_at}>{relTime(session.updated_at)}</time>
              {session.worktree && <span title={`Worktree ${session.worktree.branch}`} className="inline-flex"><Icon name="branch" size={12} /><span className="sr-only">Worktree isolato</span></span>}
            </span>
          </button>
          <div className="ws-session-actions">
            {/* The title names the session; the buttons are described by it (their own names stay short). */}
            <IconButton aria-label="Rinomina la sessione" aria-describedby={titleId} icon="edit" size={15} onClick={onRename} />
            <IconButton aria-label="Elimina la sessione" aria-describedby={titleId} icon="trash" size={15} onClick={onDelete} />
          </div>
        </>
      )}
    </li>
  );
}
