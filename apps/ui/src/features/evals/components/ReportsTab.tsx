import type { UseQueryResult } from "@tanstack/react-query";
import { Link, setQuery, useQueryParams } from "../../../app/router";
import { Button, DataTable, EmptyState, QueryState, StatusPill, type Column } from "../../../shared/ui";
import { formatDateTime, relTime } from "../../../shared/lib/format";
import type { EvalReport, EvalSuite } from "../../../shared/api/types";
import { pct, reportWho } from "../model/evals";
import { ReportDetail } from "./ReportDetail";

import { t, tNode, t as tr } from "../../../shared/i18n";
/** Reports of the runs, newest first; the open one (`?report=`) is detailed below the table. */
export function ReportsTab({ reports, suites }: { reports: UseQueryResult<EvalReport[]>; suites: EvalSuite[] }) {
  const query = useQueryParams();
  const open = query.report;
  const columns: Column<EvalReport>[] = [
    { key: "suite", header: tr("evals.suite"), render: (r) => <><b>{r.suite_name}</b><div className="faint text-xs mono">{r.id}</div></> },
    { key: "status", header: tr("evals.stato"), render: (r) => <StatusPill status={r.status} />, nowrap: true },
    {
      key: "rate", header: tr("evals.riusciti_2"), className: "num", nowrap: true,
      render: (r) => (r.summary.total === 0 ? "—" : <><b>{pct(r.summary.pass_rate)}</b> <span className="faint text-xs">{r.summary.passed}/{r.summary.total}</span></>),
    },
    { key: "score", header: tr("evals.score_2"), className: "num", render: (r) => (r.summary.total === 0 ? "—" : r.summary.score.toFixed(2).replace(".", ",")) },
    { key: "who", header: tr("evals.agente_profilo"), render: (r) => <span className="text-sm">{reportWho(r)}</span> },
    { key: "when", header: tr("evals.avviata"), nowrap: true, render: (r) => <span title={formatDateTime(r.started_at)}>{relTime(r.started_at)}</span> },
  ];
  return (
    <QueryState
      query={reports} isEmpty={(r) => r.length === 0}
      empty={<EmptyState icon="check" title={t("evals.nessun_report")} description={t("evals.esegui_una_suite_dalla_scheda")} action={<Link to="/evals/suites" className="btn primary">{t("evals.vai_alle_suite")}</Link>} />}
    >
      {(all) => (
        <div className="stack stack-lg">
          <DataTable
            label={t("evals.report_di_valutazione")} columns={columns} rows={all} rowKey={(r) => r.id}
            onRowClick={(r) => setQuery({ report: r.id, b: undefined })}
          />
          {open ? (
            <ReportDetail key={open} reportId={open} reports={all} suites={suites} compareWith={query.b} />
          ) : (
            <p className="faint text-sm">{tNode("evals.pick_report", { open: <Button size="sm" onClick={() => setQuery({ report: all[0].id })}>{t("evals.apri_il_piu_recente")}</Button> })}</p>
          )}
        </div>
      )}
    </QueryState>
  );
}
