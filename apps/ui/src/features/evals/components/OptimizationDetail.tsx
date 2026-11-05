import { useState } from "react";
import { Link } from "../../../app/router";
import { Button, DataTable, DecisionDialog, EmptyState, Icon, KeyValue, Led, Panel, Pill, type Column } from "../../../shared/ui";
import { formatDateTime } from "../../../shared/lib/format";
import type { PromptOptimization } from "../../../shared/api/types";
import { useApplyOptimization } from "../api";
import { pct } from "../model/evals";
import { canApply, defaultVariant, diffLineKind, isOptimizing, optRows, verdict, type OptRow } from "../model/optimizations";

const score = (n: number) => n.toFixed(2).replace(".", ",");
const signed = (n: number) => `${n > 0 ? "+" : n < 0 ? "−" : ""}${Math.abs(n).toFixed(2).replace(".", ",")}`;

function Diff({ text }: { text: string }) {
  if (!text) return <p className="faint text-sm">Il prompt è uguale a quello attuale.</p>;
  return (
    <pre className="ev-diff" aria-label="Differenze del prompt" tabIndex={0}>
      {text.replace(/\n$/, "").split("\n").map((line, i) => <span key={i} className={diffLineKind(line, i)}>{line || " "}</span>)}
    </pre>
  );
}

/** One optimization: baseline against variants, the diff of the prompt of the variant picked, and the apply button of the best one. */
export function OptimizationDetail({ id, list, suiteName }: { id: string; list: PromptOptimization[]; suiteName: (id: string) => string }) {
  const o = list.find((x) => x.id === id);
  const apply = useApplyOptimization();
  const [picked, setPicked] = useState<number | null | undefined>(undefined);
  const [confirming, setConfirming] = useState(false);
  if (!o) return <EmptyState icon="sparkle" title="Ottimizzazione non trovata" description="Potrebbe essere stata rimossa o l'elenco è cambiato." />;

  const shown = picked === undefined ? defaultVariant(o) : picked;
  const variant = shown === null ? undefined : o.variants[shown];
  const columns: Column<OptRow>[] = [
    { key: "label", header: "Prompt", render: (r) => <span className={r.best ? "ev-opt-best" : undefined}>{r.label}{r.best && <> <Pill tone="ok">migliore</Pill></>}{r.applied && <> <Pill tone="info">applicata</Pill></>}</span> },
    { key: "score", header: "Score", className: "num", render: (r) => score(r.score) },
    { key: "rate", header: "Riusciti", className: "num", render: (r) => pct(r.passRate) },
    { key: "delta", header: "Rispetto all'attuale", className: "num", render: (r) => (r.delta === undefined ? "—" : <span className="ev-delta" data-tone={r.delta > 0 ? "better" : r.delta < 0 ? "worse" : "same"}>{signed(r.delta)}</span>) },
    {
      key: "report", header: "Report", render: (r) => (r.reportId ? <Link to={`/evals/reports?report=${encodeURIComponent(r.reportId)}`} className="mono text-xs">{r.reportId}</Link> : "—"),
    },
  ];
  const rows = optRows(o);
  return (
    <div className="stack stack-lg ev-report">
      <Panel title={`${suiteName(o.suite_id)} · ${o.agent_id}`} icon="sparkle">
        <KeyValue items={[
          ["Stato", <span key="s" className="row">{isOptimizing(o) && <Led tone="run" small />}{verdict(o)}</span>],
          ["Avviata", formatDateTime(o.started_at)],
          o.finished_at ? ["Finita", formatDateTime(o.finished_at)] : null,
        ]} />
        {isOptimizing(o) && <p className="row text-sm" role="status"><Led tone="run" small /> La pagina si aggiorna da sola mentre le varianti vengono provate.</p>}
        {o.error && <p className="run-error" role="alert"><Icon name="alert" size={14} />{o.error}</p>}
      </Panel>

      <Panel title="Prompt a confronto" icon="list">
        <DataTable
          className="ev-opt-table" label="Punteggio del prompt attuale e delle varianti" columns={columns} rows={rows} rowKey={(r) => String(r.key)}
          onRowClick={(r) => r.key !== "base" && setPicked(r.key)}
        />
      </Panel>

      {variant && shown !== null && shown !== undefined && (
        <Panel
          title={`Variante ${shown + 1}`} icon="edit"
          actions={canApply(o, shown) && <Button variant="primary" icon="check" onClick={() => setConfirming(true)}>Applica all'agente</Button>}
        >
          <p className="muted text-sm">{variant.rationale || "Nessuna motivazione."}</p>
          <Diff text={variant.diff} />
          {o.status === "done" && o.best !== shown && (
            <p className="faint text-xs">{o.best === undefined ? "Nessuna variante batte il prompt attuale: nulla da applicare." : `Si propone la variante ${o.best + 1}, quella che fa meglio.`}</p>
          )}
        </Panel>
      )}

      {confirming && variant && shown !== null && shown !== undefined && (
        <DecisionDialog
          title={`Applicare la variante ${shown + 1} a «${o.agent_id}»?`} tone="primary" confirmLabel="Applica" icon="check" pending={apply.isPending} error={apply.error ?? undefined}
          onCancel={() => { setConfirming(false); apply.reset(); }}
          onConfirm={() => apply.mutate({ id: o.id, variant: shown }, { onSuccess: () => setConfirming(false) })}
        >
          <p>Il system prompt dell'agente diventa quello della variante e vale da subito per tutte le sue conversazioni. Le differenze sono quelle che vedi sopra.</p>
        </DecisionDialog>
      )}
    </div>
  );
}
