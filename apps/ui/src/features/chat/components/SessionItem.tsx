import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { cx } from "../../../shared/lib/cx";
import { relTime } from "../../../shared/lib/format";
import { Icon, IconButton, Led, type LedTone } from "../../../shared/ui";
import type { Session } from "../../../shared/api/types";
import { RUN_STATUS_LABEL } from "../model/labels";
import { MAX_TITLE, validateTitle } from "../model/sessions";
import { useRenameSession } from "../api";

import { t } from "../../../shared/i18n";
const CHANNEL_NAME = { telegram: "Telegram", slack: "Slack", email: "Email" } as const;

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
          aria-label={t("chat.nuovo_titolo_della_sessione")}
          aria-invalid={error ? true : undefined}
          value={value}
          maxLength={MAX_TITLE}
          disabled={rename.isPending}
          onChange={(e) => {
            setValue(e.target.value);
            setError(null);
          }}
        />
        <IconButton type="submit" aria-label={t("chat.salva_il_titolo")} icon="check" size={16} disabled={rename.isPending} />
        <IconButton aria-label={t("chat.annulla_la_modifica")} icon="close" size={16} onClick={onDone} />
      </div>
      {error && <span className="field-error" role="alert">{error}</span>}
    </form>
  );
}

/** One session of the list: open it, rename it (inline) or delete it (the parent asks to confirm). */
export function SessionItem({ session, agentName, active, renaming, reflecting, onSelect, onRename, onRenamed, onReflect, onDelete }: {
  session: Session;
  agentName: string;
  active: boolean;
  renaming: boolean;
  /** The agent is writing a lesson on this conversation. */
  reflecting: boolean;
  onSelect: () => void;
  onRename: () => void;
  onRenamed: () => void;
  onReflect: () => void;
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
              {session.channel && <span className="pill neutral" title={t("chat.conversazione_che_arriva_da", { p: CHANNEL_NAME[session.channel] })}>{CHANNEL_NAME[session.channel]}</span>}
              {session.worktree && <span title={t("chat.worktree", { branch: session.worktree.branch })} className="inline-flex"><Icon name="branch" size={12} /><span className="sr-only">{t("chat.worktree_isolato_2")}</span></span>}
            </span>
          </button>
          <div className="ws-session-actions">
            {/* The title names the session; the buttons are described by it (their own names stay short). */}
            <IconButton aria-label={t("chat.rinomina_la_sessione")} aria-describedby={titleId} icon="edit" size={15} onClick={onRename} />
            <IconButton
              aria-label={t("chat.rifletti_su_questa_conversazione")} aria-describedby={titleId} icon="sparkle" size={15} disabled={reflecting || session.last_status === "running"}
              title={reflecting ? t("chat.l_agente_sta_scrivendo_una") : t("chat.rifletti_su_questa_conversazione_l")} onClick={onReflect}
            />
            <IconButton aria-label={t("chat.elimina_la_sessione")} aria-describedby={titleId} icon="trash" size={15} onClick={onDelete} />
          </div>
        </>
      )}
    </li>
  );
}
