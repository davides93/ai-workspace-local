import { DataTable, QueryState, type Column } from "../../../shared/ui";
import { formatCompact, formatDuration } from "../../../shared/lib/format";
import type { EvalComparison, EvalSide, EvalTaskDelta } from "../../../shared/api/types";
import { useEvalComparison } from "../api";
import { deltaTone, pct, pointsDelta, scoreDelta } from "../model/evals";

const side = (s: EvalSide | null | undefined) =>
  s ? <><b>{pct(s.pass_rate)}</b> <span className="faint text-xs">{s.passed}/{s.attempts} · {s.score.toFixed(2).replace(".", ",")}</span></> : <span className="faint">non eseguito</span>;

const delta = (d: number | null | undefined, text: string) => <span className="ev-delta" data-tone={deltaTone(d) ?? "none"}>{text}</span>;

/** Task by task, report A against report B: pass rate, score and the difference. */
export function ComparePanel({ a, b }: { a: string; b: string }) {
  const query = useEvalComparison(a, b);
  const columns: Column<EvalTaskDelta>[] = [
    { key: "task", header: "Task", render: (t) => <span className="mono text-sm">{t.task_id}</span> },
    { key: "a", header: "A", render: (t) => side(t.a), nowrap: true },
    { key: "b", header: "B", render: (t) => side(t.b), nowrap: true },
    { key: "dp", header: "Δ riusciti", className: "num", nowrap: true, render: (t) => delta(t.pass_rate_delta, pointsDelta(t.pass_rate_delta)) },
    { key: "ds", header: "Δ score", className: "num", nowrap: true, render: (t) => delta(t.score_delta, scoreDelta(t.score_delta)) },
  ];
  return (
    <QueryState query={query}>
      {(c: EvalComparison) => (
        <div className="stack ev-compare">
          <div className="ev-totals" role="group" aria-label="Totali del confronto">
            <div><span className="faint text-xs">Riusciti A → B</span><b>{pct(c.total_a.pass_rate)} → {pct(c.total_b.pass_rate)}</b>{delta(c.pass_rate_delta, pointsDelta(c.pass_rate_delta))}</div>
            <div><span className="faint text-xs">Score A → B</span><b>{c.total_a.score.toFixed(2).replace(".", ",")} → {c.total_b.score.toFixed(2).replace(".", ",")}</b>{delta(c.score_delta, scoreDelta(c.score_delta))}</div>
            <div><span className="faint text-xs">Token medi per prova</span><b>{formatCompact(c.total_a.avg_tokens)} → {formatCompact(c.total_b.avg_tokens)}</b></div>
            <div><span className="faint text-xs">Durata media</span><b>{formatDuration(c.total_a.avg_duration_ms / 1000)} → {formatDuration(c.total_b.avg_duration_ms / 1000)}</b></div>
          </div>
          <DataTable label="Confronto per task" columns={columns} rows={c.tasks} rowKey={(t) => t.task_id} />
        </div>
      )}
    </QueryState>
  );
}
