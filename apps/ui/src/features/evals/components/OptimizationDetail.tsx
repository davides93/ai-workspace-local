import { useState } from "react";
import { Link } from "../../../app/router";
import { Button, DataTable, DecisionDialog, EmptyState, Icon, KeyValue, Led, Panel, Pill, type Column } from "../../../shared/ui";
import { formatDateTime } from "../../../shared/lib/format";
import type { PromptOptimization } from "../../../shared/api/types";
import { useApplyOptimization } from "../api";
import { pct } from "../model/evals";
import { canApply, defaultVariant, diffLineKind, isOptimizing, optRows, verdict, type OptRow } from "../model/optimizations";

import { t, t as tr } from "../../../shared/i18n";
const score = (n: number) => n.toFixed(2).replace(".", ",");
const signed = (n: number) => `${n > 0 ? "+" : n < 0 ? "−" : ""}${Math.abs(n).toFixed(2).replace(".", ",")}`;

function Diff({ text }: { text: string }) {
  if (!text) return <p className="faint text-sm">{t("evals.il_prompt_e_uguale_a")}</p>;
  return (
    <pre className="ev-diff" aria-label={t("evals.differenze_del_prompt")} tabIndex={0}>
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
  if (!o) return <EmptyState icon="sparkle" title={t("evals.ottimizzazione_non_trovata")} description={t("evals.potrebbe_essere_stata_rimossa_o")} />;

  const shown = picked === undefined ? defaultVariant(o) : picked;
  const variant = shown === null ? undefined : o.variants[shown];
  const columns: Column<OptRow>[] = [
    { key: "label", header: tr("evals.prompt"), render: (r) => <span className={r.best ? "ev-opt-best" : undefined}>{r.label}{r.best && <> <Pill tone="ok">{t("evals.migliore")}</Pill></>}{r.applied && <> <Pill tone="info">{t("evals.applicata")}</Pill></>}</span> },
    { key: "score", header: tr("evals.score_2"), className: "num", render: (r) => score(r.score) },
    { key: "rate", header: tr("evals.riusciti_2"), className: "num", render: (r) => pct(r.passRate) },
    { key: "delta", header: tr("evals.rispetto_all_attuale"), className: "num", render: (r) => (r.delta === undefined ? "—" : <span className="ev-delta" data-tone={r.delta > 0 ? "better" : r.delta < 0 ? "worse" : "same"}>{signed(r.delta)}</span>) },
    {
      key: "report", header: tr("evals.report"), render: (r) => (r.reportId ? <Link to={`/evals/reports?report=${encodeURIComponent(r.reportId)}`} className="mono text-xs">{r.reportId}</Link> : "—"),
    },
  ];
  const rows = optRows(o);
  return (
    <div className="stack stack-lg ev-report">
      <Panel title={`${suiteName(o.suite_id)} · ${o.agent_id}`} icon="sparkle">
        <KeyValue items={[
          [t("evals.stato"), <span key="s" className="row">{isOptimizing(o) && <Led tone="run" small />}{verdict(o)}</span>],
          [t("evals.avviata"), formatDateTime(o.started_at)],
          o.finished_at ? [t("evals.finita"), formatDateTime(o.finished_at)] : null,
        ]} />
        {isOptimizing(o) && <p className="row text-sm" role="status"><Led tone="run" small /> {t("evals.la_pagina_si_aggiorna_da")}</p>}
        {o.error && <p className="run-error" role="alert"><Icon name="alert" size={14} />{o.error}</p>}
      </Panel>

      <Panel title={t("evals.prompt_a_confronto")} icon="list">
        <DataTable
          className="ev-opt-table" label={t("evals.punteggio_del_prompt_attuale_e")} columns={columns} rows={rows} rowKey={(r) => String(r.key)}
          onRowClick={(r) => r.key !== "base" && setPicked(r.key)}
        />
      </Panel>

      {variant && shown !== null && shown !== undefined && (
        <Panel
          title={t("evals.variante", { number: shown + 1 })} icon="edit"
          actions={canApply(o, shown) && <Button variant="primary" icon="check" onClick={() => setConfirming(true)}>{t("evals.applica_all_agente")}</Button>}
        >
          <p className="muted text-sm">{variant.rationale || t("evals.nessuna_motivazione")}</p>
          <Diff text={variant.diff} />
          {o.status === "done" && o.best !== shown && (
            <p className="faint text-xs">{o.best === undefined ? t("evals.nessuna_variante_batte_il_prompt") : t("evals.si_propone_la_variante_quella", { number: o.best + 1 })}</p>
          )}
        </Panel>
      )}

      {confirming && variant && shown !== null && shown !== undefined && (
        <DecisionDialog
          title={t("evals.applicare_la_variante_a", { number: shown + 1, agent_id: o.agent_id })} tone="primary" confirmLabel={t("evals.applica")} icon="check" pending={apply.isPending} error={apply.error ?? undefined}
          onCancel={() => { setConfirming(false); apply.reset(); }}
          onConfirm={() => apply.mutate({ id: o.id, variant: shown }, { onSuccess: () => setConfirming(false) })}
        >
          <p>{t("evals.il_system_prompt_dell_agente")}</p>
        </DecisionDialog>
      )}
    </div>
  );
}
