import { useEffect, useId, useRef, useState } from "react";
import { Link } from "../../../app/router";
import { errorMessage, errorStatus } from "../../../shared/lib/errors";
import { formatDateTime, relTime } from "../../../shared/lib/format";
import { Button, DecisionDialog, Icon, Pill } from "../../../shared/ui";
import type { ApprovalRequest, BackgroundTask } from "../../../shared/api/types";
import { ApprovalCard } from "../../approvals/components/ApprovalCard";
import type { ResumeRequest } from "../api";
import { ADD_TIME, commands, instructionInfo, pauseSentence, progressLine, statusInfo, stoppingSentence } from "../model/tasks";
import { TaskInstructionBox } from "./TaskInstructionBox";

import { t } from "../../../shared/i18n";
/** Per-call callbacks of a mutation (the screen reacts to how one command ended). */
export interface Done {
  onSuccess?: () => void;
  onError?: (e: unknown) => void;
  onSettled?: () => void;
}

/** What the cards can ask for: one function per command, each with the callbacks of that call. */
export interface TaskActions {
  pause: (id: string, done?: Done) => void;
  resume: (v: ResumeRequest, done?: Done) => void;
  cancel: (id: string, done?: Done) => void;
  send: (v: { id: string; text: string; source: "user" | "voice" }, done?: Done) => void;
  respond: (request: ApprovalRequest, decision: "allow" | "deny") => void;
}

/** What the person said to a task and what became of it. */
export function TaskInstructions({ task }: { task: BackgroundTask }) {
  if (task.instructions.length === 0) return null;
  return (
    <ul className="tk-instr" aria-label={t("inbox.task_instructions_title")}>
      {task.instructions.map((i) => {
        const info = instructionInfo(i, task);
        return (
          <li key={i.id} className="tk-instr-item">
            <span className="tk-instr-text">{i.text}</span>
            <span className="tk-instr-meta">
              <Pill tone={info.tone}>{info.label}</Pill>
              {i.source === "voice" && <span className="faint text-xs">{t("inbox.task_instr_voice")}</span>}
              <time className="faint text-xs" dateTime={i.created_at}>{relTime(i.created_at)}</time>
            </span>
            {i.note && <span className="muted text-xs tk-instr-note">{i.note}</span>}
          </li>
        );
      })}
    </ul>
  );
}

/** The task's own history, closed until asked for. */
export function TaskNotes({ task }: { task: BackgroundTask }) {
  if (task.notes.length === 0) return null;
  return (
    <details className="tk-notes">
      <summary>{t("inbox.task_notes", { n: task.notes.length })}</summary>
      <ol className="tk-notes-list">
        {task.notes.map((n, i) => (
          <li key={`${n.at}-${i}`}>
            <time className="faint text-xs" dateTime={n.at}>{formatDateTime(n.at)}</time> <span className="text-sm">{n.text}</span>
          </li>
        ))}
      </ol>
    </details>
  );
}

/**
 * A task that is not finished: its state, why it is paused, how far it got, the confirmation it waits for, the commands
 * that make sense now and the box to tell it something. It never acts alone: whatever it wants to write asks as always.
 */
export function TaskCard({ task, busy, answering, actions }: {
  task: BackgroundTask;
  busy: boolean;
  /** The id of the confirmation whose answer is on its way. */
  answering?: string;
  actions: TaskActions;
}) {
  const id = useId();
  const [refusal, setRefusal] = useState<string>();
  const [asking, setAsking] = useState(false);
  const status = statusInfo(task);
  // The engine's refusal of a resume stays in front of the person until the task moves on.
  const refused = task.status === "paused" || task.status === "queued" ? refusal : undefined;
  const can = commands(task, { resumeRefused: !!refused });
  const sentence = task.status === "running" && task.stopping ? stoppingSentence(task) : pauseSentence(task);
  const card = useRef<HTMLElement>(null);
  // The button that was pressed goes away with the state it belonged to: the keyboard stays on the card.
  const keepFocus = () => card.current?.focus();
  // A refusal belongs to the pause it answered: once the task works again (the person's resume, or its own) it is gone.
  useEffect(() => {
    if (task.status === "running") setRefusal(undefined);
  }, [task.status]);
  const resume = (v: Omit<ResumeRequest, "id">) =>
    actions.resume({ id: task.id, ...v }, {
      onSuccess: () => {
        setRefusal(undefined);
        keepFocus();
      },
      onError: (e) => setRefusal(errorStatus(e) === 409 ? errorMessage(e) : undefined),
    });
  const { progress } = task;
  return (
    <article ref={card} tabIndex={-1} className="tk-card" aria-labelledby={`${id}-t`} data-task={task.id} data-status={task.status}>
      <header className="tk-head">
        <b id={`${id}-t`} className="tk-title">{task.title}</b>
        <Pill tone={status.tone}>{status.label}</Pill>
        <time className="faint text-xs" dateTime={task.updated_at}>{relTime(task.updated_at)}</time>
      </header>
      {sentence && <p className="tk-pause text-sm"><Icon name={task.stopping ? "pause" : "clock"} size={14} /><span>{sentence}</span></p>}
      {refused && <p className="tk-refusal text-sm"><Icon name="alert" size={14} /><span>{refused}</span></p>}
      <p className="muted text-sm tk-progress">{progressLine(progress)}</p>
      {progress.last_text && <p className="text-sm tk-said"><span className="faint">{t("inbox.task_last_text")}{" "}</span>{progress.last_text}</p>}
      {task.waiting_approval && (
        <div className="tk-ask" role="group" aria-label={t("inbox.task_waiting_group")}>
          <ApprovalCard
            request={task.waiting_approval} variant="compact" pending={answering === task.waiting_approval.id}
            onRespond={(decision) => actions.respond(task.waiting_approval!, decision)}
          />
        </div>
      )}
      <div className="tk-actions">
        {can.pause && <Button size="sm" icon="pause" disabled={busy} onClick={() => actions.pause(task.id, { onSuccess: keepFocus })}>{t("inbox.task_pause")}</Button>}
        {can.resume && <Button size="sm" variant="primary" icon="play" disabled={busy} onClick={() => resume({})}>{t("inbox.task_resume")}</Button>}
        {can.resumeAnyway && <Button size="sm" icon="bolt" disabled={busy} onClick={() => resume({ force: true })}>{t("inbox.task_resume_anyway")}</Button>}
        {can.addTime && <Button size="sm" variant="primary" icon="plus" disabled={busy} onClick={() => resume({ extend: { ...ADD_TIME } })}>{t("inbox.task_add_time")}</Button>}
        {can.addTimeAnyway && <Button size="sm" icon="bolt" disabled={busy} onClick={() => resume({ force: true, extend: { ...ADD_TIME } })}>{t("inbox.task_add_time_anyway")}</Button>}
        {can.cancel && <Button size="sm" variant="danger" icon="close" disabled={busy} onClick={() => setAsking(true)}>{t("inbox.task_cancel")}</Button>}
        <Link to={`/chat/${encodeURIComponent(task.session_id)}`} className="text-sm tk-chat">{t("inbox.apri_la_chat")}</Link>
      </div>
      {can.instruct && (
        <TaskInstructionBox busy={busy} onSend={(text, source, sent) => actions.send({ id: task.id, text, source }, { onSuccess: sent })} />
      )}
      <TaskInstructions task={task} />
      <TaskNotes task={task} />
      {asking && (
        <DecisionDialog
          title={t("inbox.task_cancel_title")} confirmLabel={t("inbox.task_cancel_confirm")} cancelLabel={t("inbox.task_cancel_keep")} icon="close"
          pending={busy} onCancel={() => setAsking(false)} onConfirm={() => actions.cancel(task.id, { onSettled: () => setAsking(false) })}
        >
          <p>{t("inbox.task_cancel_body", { title: task.title })}</p>
        </DecisionDialog>
      )}
    </article>
  );
}

/** A finished task: how it ended, and on request what was said to it and its history (it takes no more instructions). */
export function TaskDoneRow({ task }: { task: BackgroundTask }) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const status = statusInfo(task);
  const ended = task.error ?? task.result;
  return (
    <li className="tk-done" data-task={task.id} data-status={task.status}>
      <div className="tk-done-head">
        <Pill tone={status.tone}>{status.label}</Pill>
        <span id={`${id}-t`} className="tk-done-title">{task.title}</span>
        <Link to={`/chat/${encodeURIComponent(task.session_id)}`} className="text-sm">{t("inbox.apri_la_chat")}</Link>
        <time className="faint text-xs" dateTime={task.finished_at ?? task.updated_at}>{relTime(task.finished_at ?? task.updated_at)}</time>
      </div>
      {ended && <p className="text-sm muted tk-done-text"><span className="faint">{task.error ? t("inbox.task_error") : t("inbox.task_result")}{" "}</span>{ended}</p>}
      <details className="tk-more" onToggle={(e) => setOpen(e.currentTarget.open)}>
        <summary aria-describedby={`${id}-t`}>{t("inbox.task_details")}</summary>
        {open && (
          <div className="tk-more-body">
            <p className="muted text-sm">{progressLine(task.progress)}</p>
            <TaskInstructions task={task} />
            <TaskNotes task={task} />
            <TaskInstructionBox busy={false} disabled hint={t("inbox.task_finished_no_instructions")} onSend={() => undefined} />
          </div>
        )}
      </details>
    </li>
  );
}
