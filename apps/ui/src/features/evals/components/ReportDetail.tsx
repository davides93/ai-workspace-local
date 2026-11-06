import { Link, setQuery } from "../../../app/router";
import { CopyButton, EmptyState, Icon, KeyValue, Led, Panel, QueryState, Select, Field } from "../../../shared/ui";
import { formatDateTime, formatDuration } from "../../../shared/lib/format";
import type { EvalReport, EvalSuite } from "../../../shared/api/types";
import { useEvalReport } from "../api";
import { comparableWith, isRunning, pct, progress, reportWho, taskRows, type TaskRow } from "../model/evals";
import { ComparePanel } from "./ComparePanel";

function TaskBlock({ row }: { row: TaskRow }) {
  return (
    <li className="ev-task" data-passed={row.passed === row.attempts}>
      <div className="ev-task-head">
        <Icon name={row.passed === row.attempts ? "check" : "alert"} size={15} />
        <b className="mono text-sm">{row.task_id}</b>
        <span className="num text-sm">{row.passed}/{row.attempts} · {pct(row.pass_rate)}</span>
        <span className="faint text-xs num">score {row.score.toFixed(2).replace(".", ",")} · {Math.round(row.avg_tokens)} token · {formatDuration(row.avg_duration_ms / 1000)}</span>
      </div>
      {row.results.map((t) => (
        <details key={t.attempt} className="ev-attempt" open={!t.passed}>
          <summary>
            <span className={t.passed ? "ev-ok" : "ev-ko"}>{t.passed ? "riuscito" : "non riuscito"}</span>
            {row.attempts > 1 && <span className="faint"> · tentativo {t.attempt}</span>}
            <span className="faint"> · {t.turns} turni</span>
          </summary>
          {t.error && <p className="run-error" role="alert"><Icon name="alert" size={14} />{t.error}</p>}
          <ul className="ev-graders" aria-label="Grader">
            {t.graders.map((g, i) => (
              <li key={i} data-passed={g.passed}>
                <Led tone={g.passed ? "ok" : "crit"} small />
                <span className="mono text-xs">{g.kind}</span>
                <span className="text-sm">{g.detail}</span>
                {g.kind === "judge" && <span className="faint text-xs num">{g.score.toFixed(2).replace(".", ",")}</span>}
              </li>
            ))}
            {t.graders.length === 0 && <li className="faint text-sm">Nessun grader applicato.</li>}
          </ul>
          {t.run_id && (
            <p className="faint text-xs row">
              Run <span className="mono">{t.run_id}</span> <CopyButton text={t.run_id} label="Copia id" />
              <Link to="/activity">Cerca in Attività</Link>
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
    <QueryState query={query} empty={<EmptyState icon="check" title="Report non trovato" />}>
      {(r) => {
        const suite = suites.find((s) => s.id === r.suite_id);
        const p = progress(r, suite);
        const secs = r.finished_at ? (Date.parse(r.finished_at) - Date.parse(r.started_at)) / 1000 : null;
        const others = comparableWith(r, reports);
        return (
          <div className="stack stack-lg ev-report">
            <Panel title={`${r.suite_name}`} icon="check">
              <KeyValue items={[
                ["Stato", <span key="s" className="row">{isRunning(r) && <Led tone="run" small />}{r.status === "running" ? "in corso" : r.status === "failed" ? "interrotta" : "completata"}</span>],
                ["Riusciti", r.summary.total ? `${r.summary.passed} su ${r.summary.total} (${pct(r.summary.pass_rate)})` : "—"],
                ["Score medio", r.summary.total ? r.summary.score.toFixed(2).replace(".", ",") : "—"],
                ["Agente · profilo", reportWho(r)],
                r.repeat > 1 && ["Ripetizioni", `${r.repeat} per task`],
                ["Avviata", formatDateTime(r.started_at)],
                secs !== null && ["Durata", formatDuration(secs)],
              ]} />
              {isRunning(r) && (
                <p className="row text-sm" role="status"><Led tone="run" small /> In corso: {p.done}{p.total ? ` di ${p.total}` : ""} prove completate. La pagina si aggiorna da sola.</p>
              )}
              {r.error && <p className="run-error" role="alert"><Icon name="alert" size={14} />{r.error}</p>}
            </Panel>

            <Panel title="Task" icon="list">
              {r.tasks.length === 0 ? <p className="faint text-sm">{isRunning(r) ? "Il primo task è in corso…" : "Nessun task eseguito."}</p> : (
                <ul className="ev-task-list" aria-label="Task del report">{taskRows(r).map((row) => <TaskBlock key={row.task_id} row={row} />)}</ul>
              )}
            </Panel>

            <Panel title="Confronto A/B" icon="refresh">
              {others.length === 0 ? (
                <p className="faint text-sm">Per confrontare servono altri report della stessa suite, per esempio eseguita con un altro profilo di inferenza.</p>
              ) : (
                <div className="stack">
                  <Field label="Confronta con (B)" htmlFor="ev-compare" hint={`A è questo report (${reportWho(r)}); le differenze sono B meno A.`}>
                    <Select id="ev-compare" value={compareWith ?? ""} onChange={(e) => setQuery({ b: e.target.value })}>
                      <option value="">Scegli un report…</option>
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
