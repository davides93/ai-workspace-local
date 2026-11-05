import type { UseQueryResult } from "@tanstack/react-query";
import { Link, setQuery, useQueryParams } from "../../../app/router";
import { Button, DataTable, EmptyState, QueryState, StatusPill, type Column } from "../../../shared/ui";
import { formatDateTime, relTime } from "../../../shared/lib/format";
import type { EvalReport, EvalSuite } from "../../../shared/api/types";
import { pct, reportWho } from "../model/evals";
import { ReportDetail } from "./ReportDetail";

/** Reports of the runs, newest first; the open one (`?report=`) is detailed below the table. */
export function ReportsTab({ reports, suites }: { reports: UseQueryResult<EvalReport[]>; suites: EvalSuite[] }) {
  const query = useQueryParams();
  const open = query.report;
  const columns: Column<EvalReport>[] = [
    { key: "suite", header: "Suite", render: (r) => <><b>{r.suite_name}</b><div className="faint text-xs mono">{r.id}</div></> },
    { key: "status", header: "Stato", render: (r) => <StatusPill status={r.status} />, nowrap: true },
    {
      key: "rate", header: "Riusciti", className: "num", nowrap: true,
      render: (r) => (r.summary.total === 0 ? "—" : <><b>{pct(r.summary.pass_rate)}</b> <span className="faint text-xs">{r.summary.passed}/{r.summary.total}</span></>),
    },
    { key: "score", header: "Score", className: "num", render: (r) => (r.summary.total === 0 ? "—" : r.summary.score.toFixed(2).replace(".", ",")) },
    { key: "who", header: "Agente · profilo", render: (r) => <span className="text-sm">{reportWho(r)}</span> },
    { key: "when", header: "Avviata", nowrap: true, render: (r) => <span title={formatDateTime(r.started_at)}>{relTime(r.started_at)}</span> },
  ];
  return (
    <QueryState
      query={reports} isEmpty={(r) => r.length === 0}
      empty={<EmptyState icon="check" title="Nessun report" description="Esegui una suite dalla scheda Suite: qui trovi quanti task sono riusciti, il punteggio e cosa ha detto ogni grader." action={<Link to="/evals/suites" className="btn primary">Vai alle suite</Link>} />}
    >
      {(all) => (
        <div className="stack stack-lg">
          <DataTable
            label="Report di valutazione" columns={columns} rows={all} rowKey={(r) => r.id}
            onRowClick={(r) => setQuery({ report: r.id, b: undefined })}
          />
          {open ? (
            <ReportDetail key={open} reportId={open} reports={all} suites={suites} compareWith={query.b} />
          ) : (
            <p className="faint text-sm">Scegli un report per vedere i task, i grader e confrontarlo con un altro. <Button size="sm" onClick={() => setQuery({ report: all[0].id })}>Apri il più recente</Button></p>
          )}
        </div>
      )}
    </QueryState>
  );
}
