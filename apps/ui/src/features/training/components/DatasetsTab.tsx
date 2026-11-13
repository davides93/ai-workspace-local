import { useState } from "react";
import type { UseQueryResult } from "@tanstack/react-query";
import { Button, DataTable, DecisionDialog, Dialog, EmptyState, Field, Input, QueryState, Section, Select, type Column } from "../../../shared/ui";
import { formatCompact, formatDateTime, relTime } from "../../../shared/lib/format";
import { errorMessage } from "../../../shared/lib/errors";
import type { DatasetInfo, TrainDoctor, TrainMethod } from "../../../shared/api/types";
import { useAgentChoices } from "../../workflows/api";
import { useEvalSuites } from "../../evals/api";
import { useBuildDataset, useDeleteDataset } from "../api";
import { blankDatasetForm, datasetSpec, METHOD_HINT, DPO_SOURCE_LABEL, METHOD_LABEL, METHOD_SOURCES, METHODS, MIN_EXAMPLES, SOURCE_LABEL, sourceLabel, type DatasetForm } from "../model/training";

import { t } from "../../../shared/i18n";
function BuildDialog({ onClose }: { onClose: () => void }) {
  const agents = useAgentChoices();
  const suites = useEvalSuites();
  const build = useBuildDataset();
  const [f, setF] = useState<DatasetForm>(blankDatasetForm());
  const [shown, setShown] = useState<string | undefined>();
  const set = <K extends keyof DatasetForm>(k: K, v: DatasetForm[K]) => setF((x) => ({ ...x, [k]: v }));
  const toggle = (s: string) => set("sources", f.sources.includes(s) ? f.sources.filter((x) => x !== s) : [...f.sources, s]);
  /** The sources of the new method that were already ticked, or its first one. */
  const changeMethod = (m: TrainMethod) =>
    setF((x) => {
      const kept = x.sources.filter((s) => METHOD_SOURCES[m].includes(s));
      return { ...x, method: m, sources: kept.length > 0 ? kept : METHOD_SOURCES[m].slice(0, 1) };
    });
  const submit = () => {
    const r = datasetSpec(f);
    setShown(r.error);
    if (r.spec) build.mutate(r.spec, { onSuccess: onClose });
  };
  return (
    <Dialog
      title={t("training.nuovo_dataset")} size="md" onClose={onClose}
      footer={<><Button onClick={onClose}>{t("training.annulla")}</Button><Button variant="primary" onClick={submit} disabled={build.isPending}>{t("training.crea")}</Button></>}
    >
      <form className="stack stack-lg" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <p className="muted text-sm">
          {t("training.servono_almeno_esempi_i_segreti", { min: MIN_EXAMPLES })}</p>
        <Field label={t("training.metodo")} htmlFor="ds-method" hint={METHOD_HINT[f.method]}>
          <Select id="ds-method" value={f.method} onChange={(e) => changeMethod(e.target.value as TrainMethod)}>
            {METHODS.map((m) => <option key={m} value={m}>{METHOD_LABEL[m]}</option>)}
          </Select>
        </Field>
        <Field label={t("training.nome")} htmlFor="ds-name" hint={t("training.facoltativo")}><Input id="ds-name" value={f.name} onChange={(e) => set("name", e.target.value)} autoComplete="off" /></Field>
        {METHOD_SOURCES[f.method].length > 0 && (
          <fieldset className="tr-sources">
            <legend>{f.method === "dpo" ? t("training.da_dove_prendere_le_coppie") : t("training.da_dove_prendere_gli_esempi")}</legend>
            {METHOD_SOURCES[f.method].map((id) => (
              <label key={id}><input type="checkbox" checked={f.sources.includes(id)} onChange={() => toggle(id)} /> {(f.method === "dpo" ? DPO_SOURCE_LABEL : SOURCE_LABEL)[id]}</label>
            ))}
          </fieldset>
        )}
        {f.method === "grpo" && (
          <>
            <Field label={t("training.suite")} htmlFor="ds-suite" hint={t("training.i_task_danno_i_prompt")}>
              <Select id="ds-suite" value={f.suite} onChange={(e) => set("suite", e.target.value)}>
                <option value="">{t("training.scegli")}</option>
                {(suites.data ?? []).map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
              </Select>
            </Field>
            <Field label={t("training.ripetizioni")} htmlFor="ds-repeat" hint={t("training.quante_volte_compare_ogni_prompt")}>
              <Input id="ds-repeat" inputMode="numeric" value={f.repeat} onChange={(e) => set("repeat", e.target.value)} placeholder="1" />
            </Field>
          </>
        )}
        {f.sources.includes("eval") && (
          <Field label={t("training.report_di_valutazione")} htmlFor="ds-reports" hint={t("training.gli_id_dei_report_separati")}>
            <Input id="ds-reports" value={f.evalReports} onChange={(e) => set("evalReports", e.target.value)} mono autoComplete="off" />
          </Field>
        )}
        <Field label={t("training.agente")} htmlFor="ds-agent" hint={t("training.solo_le_sessioni_di_questo")}>
          <Select id="ds-agent" value={f.agent} onChange={(e) => set("agent", e.target.value)}>
            <option value="">{t("training.tutti")}</option>
            {(agents.data ?? []).map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </Select>
        </Field>
        <div className="tr-form-grid">
          <Field label={t("training.dal")} htmlFor="ds-since"><Input id="ds-since" type="date" value={f.since} onChange={(e) => set("since", e.target.value)} /></Field>
          <Field label={t("training.al")} htmlFor="ds-until"><Input id="ds-until" type="date" value={f.until} onChange={(e) => set("until", e.target.value)} /></Field>
          <Field label={t("training.al_massimo")} htmlFor="ds-limit"><Input id="ds-limit" inputMode="numeric" value={f.limit} onChange={(e) => set("limit", e.target.value)} placeholder={t("training.tutti_2")} /></Field>
        </div>
        {(shown || build.isError) && <p className="tr-error" role="alert">{shown ?? errorMessage(build.error)}</p>}
        <button type="submit" className="sr-only" tabIndex={-1}>{t("training.crea")}</button>
      </form>
    </Dialog>
  );
}

/** The datasets written so far, with their size; new ones are made from the sessions, the runs and the evaluations. */
export function DatasetsTab({ datasets }: { datasets: UseQueryResult<DatasetInfo[]>; doctor: TrainDoctor | undefined }) {
  const [building, setBuilding] = useState(false);
  const [removing, setRemoving] = useState<DatasetInfo | null>(null);
  const del = useDeleteDataset();
  const columns: Column<DatasetInfo>[] = [
    { key: "name", header: t("training.dataset"), render: (d) => <><b>{d.name}</b><div className="faint text-xs mono">{d.id}</div></> },
    { key: "method", header: t("training.metodo"), render: (d) => METHOD_LABEL[d.method], nowrap: true },
    { key: "n", header: t("training.esempi"), className: "num", nowrap: true, render: (d) => <>{d.examples} <span className="faint text-xs">{d.train} + {d.valid}</span></> },
    { key: "tokens", header: t("training.token"), className: "num", nowrap: true, render: (d) => `~${formatCompact(d.tokens_est)}` },
    { key: "from", header: t("training.origine"), render: (d) => <span className="text-sm">{d.sources.map(sourceLabel).join(", ")}{d.agents.length ? ` · ${d.agents.join(", ")}` : ""}</span> },
    { key: "when", header: t("training.creato"), nowrap: true, render: (d) => <span title={formatDateTime(d.created_at)}>{relTime(d.created_at)}</span> },
    { key: "rm", header: "", render: (d) => <Button size="sm" variant="ghost" icon="trash" aria-label={t("training.elimina", { name: d.name })} onClick={() => setRemoving(d)} /> },
  ];
  return (
    <Section
      title={t("training.dataset")} hint={t("training.esempi_per_l_addestramento_il")}
      actions={<Button variant="primary" icon="plus" onClick={() => setBuilding(true)}>{t("training.nuovo_dataset")}</Button>}
    >
      <QueryState
        query={datasets} isEmpty={(d) => d.length === 0}
        empty={<EmptyState icon="file" title={t("training.nessun_dataset")} description={t("training.dai_un_pollice_in_su")} action={<Button variant="primary" onClick={() => setBuilding(true)}>{t("training.crea_il_primo_dataset")}</Button>} />}
      >
        {(all) => <DataTable label={t("training.dataset_di_addestramento")} columns={columns} rows={all} rowKey={(d) => d.id} />}
      </QueryState>
      {building && <BuildDialog onClose={() => setBuilding(false)} />}
      {removing && (
        <DecisionDialog
          title={t("training.eliminare_il_dataset")} confirmLabel={t("training.elimina_2")} icon="trash" pending={del.isPending} error={del.error ?? undefined}
          onCancel={() => { setRemoving(null); del.reset(); }} onConfirm={() => del.mutate(removing.id, { onSuccess: () => setRemoving(null) })}
        >
          <p>{t("training.esempi_viene_cancellato_dal_disco", { name: removing.name, examples: removing.examples })}</p>
        </DecisionDialog>
      )}
    </Section>
  );
}
