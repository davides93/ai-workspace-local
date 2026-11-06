import { useId, useState } from "react";
import { Link } from "../../../app/router";
import { errorMessage } from "../../../shared/lib/errors";
import { relTime } from "../../../shared/lib/format";
import { Button, Field, Input, Markdown, Panel, Pill, QueryState, Textarea } from "../../../shared/ui";
import type { Lesson } from "../../../shared/api/types";
import { MAX_LESSON_CONTENT_CHARS, MAX_LESSON_TITLE_CHARS } from "../../../shared/api/types";
import { useApproveLesson, useLessons, useRejectLesson } from "../api";
import { checkLessonEdit, editOf, KIND_DESTINATION, KIND_LABEL } from "../model/lessons";
import "../learning.css";

/** One proposal: what the agent learned and why, with a preview; the user can edit it, approve it (it is written) or discard it. */
function LessonCard({ lesson, busy, onApprove, onReject, error }: {
  lesson: Lesson;
  busy: boolean;
  error?: unknown;
  onApprove: (edit: { title?: string; content?: string }) => void;
  onReject: () => void;
}) {
  const id = useId();
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(lesson.title);
  const [content, setContent] = useState(lesson.content);
  const check = checkLessonEdit(title, content);

  const approve = () => {
    if (!check.ok) return;
    onApprove(editOf(lesson, title, content));
  };
  const cancelEdit = () => {
    setTitle(lesson.title);
    setContent(lesson.content);
    setEditing(false);
  };
  return (
    <article className="ln-card" aria-labelledby={`${id}-t`}>
      <header className="ln-head">
        <Pill tone={lesson.kind === "skill" ? "info" : "neutral"}>{KIND_LABEL[lesson.kind]}</Pill>
        <b id={`${id}-t`} className="ln-title">{lesson.title}</b>
        <time className="faint text-xs" dateTime={lesson.created_at}>{relTime(lesson.created_at)}</time>
      </header>
      <p className="muted text-sm">{lesson.reason}</p>
      {editing ? (
        <div className="stack">
          <Field label="Titolo" htmlFor={`${id}-ti`} error={!check.ok && !title.trim() ? check.error : undefined}>
            <Input id={`${id}-ti`} value={title} maxLength={MAX_LESSON_TITLE_CHARS} onChange={(e) => setTitle(e.target.value)} />
          </Field>
          <Field label="Contenuto (Markdown)" htmlFor={`${id}-co`} hint={`Fino a ${MAX_LESSON_CONTENT_CHARS} caratteri.`} error={!check.ok && title.trim() ? check.error : undefined}>
            <Textarea id={`${id}-co`} rows={8} value={content} onChange={(e) => setContent(e.target.value)} />
          </Field>
        </div>
      ) : (
        <div className="ln-preview"><Markdown text={lesson.content} /></div>
      )}
      <p className="faint text-xs">{KIND_DESTINATION[lesson.kind]} Finché non approvi, non viene scritto nulla.</p>
      {error !== undefined && error !== null && <p className="field-error" role="alert">{errorMessage(error)}</p>}
      <div className="ln-actions">
        <Button size="sm" variant="primary" icon="check" disabled={busy || !check.ok} onClick={approve}>{editing ? "Approva con le modifiche" : "Approva"}</Button>
        {editing ? (
          <Button size="sm" variant="ghost" onClick={cancelEdit} disabled={busy}>Annulla le modifiche</Button>
        ) : (
          <Button size="sm" icon="edit" onClick={() => setEditing(true)} disabled={busy}>Modifica</Button>
        )}
        <Button size="sm" variant="ghost" icon="close" disabled={busy} onClick={onReject}>Scarta</Button>
        <Link to={`/chat/${encodeURIComponent(lesson.session_id)}`} className="btn sm ghost">Vedi la conversazione</Link>
      </div>
    </article>
  );
}

/** Lessons the agents propose after a thumbs down (or on request), waiting for the user's decision. */
export function LessonsPanel() {
  const lessons = useLessons("pending");
  const approve = useApproveLesson();
  const reject = useRejectLesson();
  const busyId = approve.isPending ? approve.variables?.id : reject.isPending ? reject.variables : undefined;
  const failed = approve.isError ? approve.variables?.id : reject.isError ? reject.variables : undefined;
  const failure = approve.isError ? approve.error : reject.error;
  return (
    <Panel title="Lezioni da rivedere" icon="sparkle" actions={lessons.data && <Pill tone={lessons.data.length ? "info" : "neutral"}>{lessons.data.length}</Pill>}>
      <QueryState query={lessons}>
        {(list) =>
          list.length === 0 ? (
            <p className="muted text-sm">Nessuna lezione in attesa. Dopo un pollice giù l'agente può proporre cosa ricordare: lo decidi tu.</p>
          ) : (
            <div className="stack stack-lg">
              {list.map((l) => (
                <LessonCard
                  key={l.id} lesson={l} busy={busyId === l.id} error={failed === l.id ? failure : undefined}
                  onApprove={(edit) => approve.mutate({ id: l.id, ...edit })} onReject={() => reject.mutate(l.id)}
                />
              ))}
            </div>
          )
        }
      </QueryState>
    </Panel>
  );
}
