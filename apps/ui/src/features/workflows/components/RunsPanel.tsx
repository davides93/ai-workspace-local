import { Button, EmptyState, Icon, StatusPill } from "../../../shared/ui";
import { formatDateTime, formatDuration, relTime } from "../../../shared/lib/format";
import type { WorkflowRun } from "../../../shared/api/types";
import { runSeconds } from "../model/triggers";

import { t } from "../../../shared/i18n";
/** Number of steps finished out of the total, and the first one that failed. */
export function runProgress(run: WorkflowRun): { done: number; total: number; failed?: string } {
  return {
    done: run.steps.filter((s) => s.status === "done" || s.status === "skipped").length,
    total: run.steps.length,
    failed: run.steps.find((s) => s.status === "failed")?.id,
  };
}

/** Runs of this workflow, newest first; each row opens its detail. */
export function RunsPanel({ runs, selectedId, onOpen, onRun, runDisabled }: {
  runs: WorkflowRun[];
  selectedId?: string;
  onOpen: (id: string) => void;
  onRun: () => void;
  runDisabled?: boolean;
}) {
  if (runs.length === 0) {
    return (
      <EmptyState
        icon="play" title={t("workflows.nessuna_esecuzione")}
        description={t("workflows.quando_il_workflow_parte_manualmente")}
        action={<Button variant="primary" icon="play" onClick={onRun} disabled={runDisabled}>{t("workflows.esegui_ora")}</Button>}
      />
    );
  }
  return (
    <ul className="run-list" aria-label={t("workflows.esecuzioni_del_workflow")}>
      {runs.map((r) => {
        const p = runProgress(r);
        const secs = runSeconds(r.started_at, r.finished_at);
        return (
          <li key={r.id}>
            <button type="button" className="run-row" aria-current={r.id === selectedId} onClick={() => onOpen(r.id)}>
              <StatusPill status={r.status} />
              <span className="run-meta">
                <span className="mono text-xs">{r.id}</span>
                {r.parent_run_id && <span className="faint text-xs">sotto-workflow di <span className="mono">{r.parent_run_id}</span></span>}
                <span className="faint text-xs" title={formatDateTime(r.started_at)}>{relTime(r.started_at)}{secs !== null ? ` · ${formatDuration(secs)}` : ""}</span>
              </span>
              <span className="run-steps text-xs num">{t("workflows.steps_done", { done: p.done, total: p.total })}{p.failed ? <span className="run-failed">{" "}{t("workflows.non_riuscito", { failed: p.failed })}</span> : null}</span>
              <Icon name="forward" size={14} />
            </button>
          </li>
        );
      })}
    </ul>
  );
}
