import { useEffect, useRef, useState } from "react";
import { isNotFound } from "../../../shared/lib/errors";
import { ErrorState, Panel, SkeletonLines } from "../../../shared/ui";
import { useCancelTask, usePauseTask, useRespondTaskApproval, useResumeTask, useSendTaskInstruction, useTasks } from "../api";
import { announcements, finishedCount, finishedTasks, openTasks, snapshots, type TaskSnapshots } from "../model/tasks";
import { TaskCard, TaskDoneRow, type Done, type TaskActions } from "./TaskCard";

import { t } from "../../../shared/i18n";
/**
 * «Lavori in background»: what the assistant does while the person is elsewhere, above the proposals. Each task can be
 * paused, resumed, told something and cancelled; the confirmation a task waits for is answered here too. A hidden live
 * region says in words when a task changes state (paused, waiting for a confirmation, finished…).
 */
export function TasksPanel() {
  const tasks = useTasks();
  const pause = usePauseTask();
  const resume = useResumeTask();
  const cancel = useCancelTask();
  const send = useSendTaskInstruction();
  const approval = useRespondTaskApproval();
  const list = tasks.data;

  // What changed since the last answer, for screen readers. The first answer is the page itself, not news.
  const before = useRef<TaskSnapshots | undefined>(undefined);
  const [said, setSaid] = useState({ n: 0, text: "" });
  useEffect(() => {
    if (!list) return;
    const lines = announcements(before.current, list);
    before.current = snapshots(list);
    if (lines.length) setSaid((s) => ({ n: s.n + 1, text: lines.join(" ") }));
  }, [list]);

  // An engine without background tasks (older than this page) leaves the proposals alone on the page.
  if (!list && tasks.isError && isNotFound(tasks.error)) return null;

  // Every command is awaited by itself: the per-call callbacks of a mutation are dropped when another call of the same one
  // starts (a second task paused while the first is still answering), and the screen must still react to how each ended.
  const run = <T,>(call: Promise<T>, done?: Done) => {
    call.then(() => done?.onSuccess?.(), (e) => done?.onError?.(e)).finally(() => done?.onSettled?.());
  };
  const busy = new Set<string>(
    [pause.isPending && pause.variables, resume.isPending && resume.variables?.id, cancel.isPending && cancel.variables, send.isPending && send.variables?.id].filter(
      (id): id is string => typeof id === "string",
    ),
  );
  const actions: TaskActions = {
    pause: (id, done) => run(pause.mutateAsync(id), done),
    resume: (v, done) => run(resume.mutateAsync(v), done),
    cancel: (id, done) => run(cancel.mutateAsync(id), done),
    send: (v, done) => run(send.mutateAsync(v), done),
    respond: approval.respond,
  };
  const open = list ? openTasks(list) : [];
  const done = list ? finishedTasks(list) : [];
  const total = list ? finishedCount(list) : 0;
  return (
    <section className="tk" aria-label={t("inbox.task_panel_title")}>
      <p className="sr-only" role="status" aria-live="polite" aria-label={t("inbox.task_announcer")}>
        <span key={said.n}>{said.text}</span>
      </p>
      <Panel title={t("inbox.task_panel_title")} icon="bolt">
        <div className="stack stack-lg">
          <p className="muted text-sm tk-hint">{t("inbox.task_panel_hint")}</p>
          {!list && tasks.isPending && <SkeletonLines lines={2} />}
          {!list && tasks.isError && <ErrorState title={t("inbox.task_list_error")} error={tasks.error} onRetry={() => void tasks.refetch()} retrying={tasks.isFetching} />}
          {list && open.length === 0 && done.length === 0 && <p className="muted text-sm">{t("inbox.task_none")}</p>}
          {open.length > 0 && (
            <ul className="tk-list" aria-label={t("inbox.task_open_label")}>
              {open.map((task) => (
                <li key={task.id}>
                  <TaskCard task={task} busy={busy.has(task.id)} answering={approval.pending} actions={actions} />
                </li>
              ))}
            </ul>
          )}
          {done.length > 0 && (
            <div className="tk-finished">
              <h3 className="tk-sub">{t("inbox.task_finished_title")}</h3>
              <ul className="tk-done-list">{done.map((task) => <TaskDoneRow key={task.id} task={task} />)}</ul>
              {total > done.length && <p className="faint text-xs">{t("inbox.task_finished_latest", { shown: done.length, total })}</p>}
            </div>
          )}
        </div>
      </Panel>
    </section>
  );
}
