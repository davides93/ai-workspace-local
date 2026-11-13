import { Link, setQuery } from "../../../app/router";
import { CopyButton, EmptyState, Icon, KeyValue, Led, Panel, QueryState, Select, Field } from "../../../shared/ui";
import { formatDateTime, formatDuration } from "../../../shared/lib/format";
import type { EvalReport, EvalSuite } from "../../../shared/api/types";
import { useEvalReport } from "../api";
import { comparableWith, isRunning, pct, progress, reportWho, taskRows, type TaskRow } from "../model/evals";
import { ComparePanel } from "./ComparePanel";

import { t as tr } from "../../../shared/i18n";
function TaskBlock({ row }: { row: TaskRow }) {
  return (
    <li className="ev-task" data-passed={row.passed === row.attempts}>
      <div className="ev-task-head">
        <Icon name={row.passed === row.attempts ? "check" : "alert"} size={15} />
        <b className="mono text-sm">{row.task_id}</b>
        <span className="num text-sm">{row.passed}/{row.attempts} · {pct(row.pass_rate)}</span>
        <span className="faint text-xs num">{tr("evals.score_token", { score: row.score.toFixed(2).replace(".", ","), tokens: Math.round(row.avg_tokens), duration: formatDuration(row.avg_duration_ms / 1000) })}</span>
      </div>
      {row.results.map((t) => (
        <details key={t.attempt} className="ev-attempt" open={!t.passed}>
          <summary>
            <span className={t.passed ? "ev-ok" : "ev-ko"}>{t.passed ? tr("evals.riuscito") : tr("evals.non_riuscito")}</span>
            {row.attempts > 1 && <span className="faint"> {" "}{tr("evals.tentativo", { attempt: t.attempt })}</span>}
            <span className="faint"> {" "}{tr("evals.turni", { turns: t.turns })}</span>
          </summary>
          {t.error && <p className="run-error" role="alert"><Icon name="alert" size={14} />{t.error}</p>}
          <ul className="ev-graders" aria-label={tr("evals.grader")}>
            {t.graders.map((g, i) => (
              <li key={i} data-passed={g.passed}>
                <Led tone={g.passed ? "ok" : "crit"} small />
                <span className="mono text-xs">{g.kind}</span>
                <span className="text-sm">{g.detail}</span>
                {g.kind === "judge" && <span className="faint text-xs num">{g.score.toFixed(2).replace(".", ",")}</span>}
              </li>
            ))}
            {t.graders.length === 0 && <li className="faint text-sm">{tr("evals.nessun_grader_applicato")}</li>}
          </ul>
          {t.run_id && (
            <p className="faint text-xs row">
              {tr("evals.run_label")} <span className="mono">{t.run_id}</span> <CopyButton text={t.run_id} label={tr("evals.copia_id")} />
              <Link to="/activity">{tr("evals.cerca_in_attivita")}</Link>
            </p>
          )}
        </details>
      ))}
    </li>
  );
}

/** One report: summary, every task with the verdict of each grader, and the comparison with another report of the same suite. */
export function ReportDetail({ reportId, reports, suites, compareWith }: { reportId: string; reports: EvalReport[]; suites: EvalSuite[]; compareWith?: string }) {
  const query = useEvalReport(reportId);
  return (
    <QueryState query={query} empty={<EmptyState icon="check" title={tr("evals.report_non_trovato")} />}>
      {(r) => {
        const suite = suites.find((s) => s.id === r.suite_id);
        const p = progress(r, suite);
        const secs = r.finished_at ? (Date.parse(r.finished_at) - Date.parse(r.started_at)) / 1000 : null;
        const others = comparableWith(r, reports);
        return (
          <div className="stack stack-lg ev-report">
            <Panel title={`${r.suite_name}`} icon="check">
              <KeyValue items={[
                [tr("evals.stato"), <span key="s" className="row">{isRunning(r) && <Led tone="run" small />}{r.status === "running" ? tr("evals.in_corso") : r.status === "failed" ? tr("evals.interrotta") : tr("evals.completata")}</span>],
                [tr("evals.riusciti_2"), r.summary.total ? tr("evals.passed_of_total", { passed: r.summary.passed, total: r.summary.total, rate: pct(r.summary.pass_rate) }) : "—"],
                [tr("evals.score_medio"), r.summary.total ? r.summary.score.toFixed(2).replace(".", ",") : "—"],
                [tr("evals.agente_profilo"), reportWho(r)],
                r.repeat > 1 && [tr("evals.ripetizioni"), tr("evals.repeat_per_task", { repeat: r.repeat })],
                [tr("evals.avviata"), formatDateTime(r.started_at)],
                secs !== null && [tr("evals.durata"), formatDuration(secs)],
              ]} />
              {isRunning(r) && (
                <p className="row text-sm" role="status"><Led tone="run" small /> {(p.total ? tr("evals.in_progress_of", { done: p.done, total: p.total }) : tr("evals.in_corso_prove_completate_la", { done: p.done }))}</p>
              )}
              {r.error && <p className="run-error" role="alert"><Icon name="alert" size={14} />{r.error}</p>}
            </Panel>

            <Panel title={tr("evals.task")} icon="list">
              {r.tasks.length === 0 ? <p className="faint text-sm">{isRunning(r) ? tr("evals.il_primo_task_e_in") : tr("evals.nessun_task_eseguito")}</p> : (
                <ul className="ev-task-list" aria-label={tr("evals.task_del_report")}>{taskRows(r).map((row) => <TaskBlock key={row.task_id} row={row} />)}</ul>
              )}
            </Panel>

            <Panel title={tr("evals.confronto_a_b")} icon="refresh">
              {others.length === 0 ? (
                <p className="faint text-sm">{tr("evals.per_confrontare_servono_altri_report")}</p>
              ) : (
                <div className="stack">
                  <Field label={tr("evals.confronta_con_b")} htmlFor="ev-compare" hint={tr("evals.a_e_questo_report_le", { report: reportWho(r) })}>
                    <Select id="ev-compare" value={compareWith ?? ""} onChange={(e) => setQuery({ b: e.target.value })}>
                      <option value="">{tr("evals.scegli_un_report")}</option>
                      {others.map((o) => <option key={o.id} value={o.id}>{o.id} · {reportWho(o)} · {formatDateTime(o.started_at)}</option>)}
                    </Select>
                  </Field>
                  {compareWith && others.some((o) => o.id === compareWith) && <ComparePanel a={r.id} b={compareWith} />}
                </div>
              )}
            </Panel>
          </div>
        );
      }}
    </QueryState>
  );
}
