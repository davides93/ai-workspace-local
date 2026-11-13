import type { UseQueryResult } from "@tanstack/react-query";
import { Link, setQuery, useQueryParams } from "../../../app/router";
import { Button, DataTable, EmptyState, QueryState, StatusPill, type Column } from "../../../shared/ui";
import { formatDateTime, relTime } from "../../../shared/lib/format";
import type { EvalSuite, PromptOptimization } from "../../../shared/api/types";
import { pct } from "../model/evals";
import { OptimizationDetail } from "./OptimizationDetail";

import { t, tNode, t as tr } from "../../../shared/i18n";
/** The optimizations of the prompt, newest first; the open one (`?opt=`) is detailed below the table. */
export function OptimizationsTab({ optimizations, suites }: { optimizations: UseQueryResult<PromptOptimization[]>; suites: EvalSuite[] }) {
  const open = useQueryParams().opt;
  const suiteName = (id: string) => suites.find((s) => s.id === id)?.name ?? id;
  const columns: Column<PromptOptimization>[] = [
    { key: "suite", header: tr("evals.suite_agente"), render: (o) => <><b>{suiteName(o.suite_id)}</b><div className="faint text-xs">{o.agent_id} · <span className="mono">{o.id}</span></div></> },
    { key: "status", header: tr("evals.stato"), render: (o) => <StatusPill status={o.status} />, nowrap: true },
    {
      key: "base", header: tr("evals.prompt_attuale"), className: "num", nowrap: true,
      render: (o) => (o.status === "running" && !o.baseline_report_id ? "…" : <>{o.baseline_score.toFixed(2).replace(".", ",")} <span className="faint text-xs">{pct(o.baseline_pass_rate)}</span></>),
    },
    {
      key: "best", header: tr("evals.migliore_2"), className: "num", nowrap: true,
      render: (o) => (o.best === undefined ? "—" : <b>{o.variants[o.best].score.toFixed(2).replace(".", ",")} <span className="faint text-xs">{pct(o.variants[o.best].pass_rate)}</span></b>),
    },
    { key: "when", header: tr("evals.avviata"), nowrap: true, render: (o) => <span title={formatDateTime(o.started_at)}>{relTime(o.started_at)}</span> },
  ];
  return (
    <QueryState
      query={optimizations} isEmpty={(l) => l.length === 0}
      empty={
        <EmptyState
          icon="sparkle" title={t("evals.nessuna_ottimizzazione")}
          description={t("evals.apri_una_suite_e_scegli")}
          action={<Link to="/evals/suites" className="btn primary">{t("evals.vai_alle_suite")}</Link>}
        />
      }
    >
      {(all) => (
        <div className="stack stack-lg">
          <DataTable label={t("evals.ottimizzazioni_del_prompt")} columns={columns} rows={all} rowKey={(o) => o.id} onRowClick={(o) => setQuery({ opt: o.id })} />
          {open ? (
            <OptimizationDetail key={open} id={open} list={all} suiteName={suiteName} />
          ) : (
            <p className="faint text-sm">{tNode("evals.pick_optimization", { open: <Button size="sm" onClick={() => setQuery({ opt: all[0].id })}>{t("evals.apri_la_piu_recente")}</Button> })}</p>
          )}
        </div>
      )}
    </QueryState>
  );
}
