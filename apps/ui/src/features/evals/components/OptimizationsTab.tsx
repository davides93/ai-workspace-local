import type { UseQueryResult } from "@tanstack/react-query";
import { Link, setQuery, useQueryParams } from "../../../app/router";
import { Button, DataTable, EmptyState, QueryState, StatusPill, type Column } from "../../../shared/ui";
import { formatDateTime, relTime } from "../../../shared/lib/format";
import type { EvalSuite, PromptOptimization } from "../../../shared/api/types";
import { pct } from "../model/evals";
import { OptimizationDetail } from "./OptimizationDetail";

/** The optimizations of the prompt, newest first; the open one (`?opt=`) is detailed below the table. */
export function OptimizationsTab({ optimizations, suites }: { optimizations: UseQueryResult<PromptOptimization[]>; suites: EvalSuite[] }) {
  const open = useQueryParams().opt;
  const suiteName = (id: string) => suites.find((s) => s.id === id)?.name ?? id;
  const columns: Column<PromptOptimization>[] = [
    { key: "suite", header: "Suite · agente", render: (o) => <><b>{suiteName(o.suite_id)}</b><div className="faint text-xs">{o.agent_id} · <span className="mono">{o.id}</span></div></> },
    { key: "status", header: "Stato", render: (o) => <StatusPill status={o.status} />, nowrap: true },
    {
      key: "base", header: "Prompt attuale", className: "num", nowrap: true,
      render: (o) => (o.status === "running" && !o.baseline_report_id ? "…" : <>{o.baseline_score.toFixed(2).replace(".", ",")} <span className="faint text-xs">{pct(o.baseline_pass_rate)}</span></>),
    },
    {
      key: "best", header: "Migliore", className: "num", nowrap: true,
      render: (o) => (o.best === undefined ? "—" : <b>{o.variants[o.best].score.toFixed(2).replace(".", ",")} <span className="faint text-xs">{pct(o.variants[o.best].pass_rate)}</span></b>),
    },
    { key: "when", header: "Avviata", nowrap: true, render: (o) => <span title={formatDateTime(o.started_at)}>{relTime(o.started_at)}</span> },
  ];
  return (
    <QueryState
      query={optimizations} isEmpty={(l) => l.length === 0}
      empty={
        <EmptyState
          icon="sparkle" title="Nessuna ottimizzazione"
          description="Apri una suite e scegli «Ottimizza il prompt»: il modello scrive delle varianti del prompt di un agente, la suite le prova e qui vedi se una fa meglio."
          action={<Link to="/evals/suites" className="btn primary">Vai alle suite</Link>}
        />
      }
    >
      {(all) => (
        <div className="stack stack-lg">
          <DataTable label="Ottimizzazioni del prompt" columns={columns} rows={all} rowKey={(o) => o.id} onRowClick={(o) => setQuery({ opt: o.id })} />
          {open ? (
            <OptimizationDetail key={open} id={open} list={all} suiteName={suiteName} />
          ) : (
            <p className="faint text-sm">Scegli un'ottimizzazione per vedere le varianti e le differenze nel prompt. <Button size="sm" onClick={() => setQuery({ opt: all[0].id })}>Apri la più recente</Button></p>
          )}
        </div>
      )}
    </QueryState>
  );
}
