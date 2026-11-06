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
      title="Nuovo dataset" size="md" onClose={onClose}
      footer={<><Button onClick={onClose}>Annulla</Button><Button variant="primary" onClick={submit} disabled={build.isPending}>Crea</Button></>}
    >
      <form className="stack stack-lg" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <p className="muted text-sm">
          Servono almeno {MIN_EXAMPLES} esempi. I segreti nelle conversazioni (chiavi, token) vengono mascherati prima di scrivere i file.
        </p>
        <Field label="Metodo" htmlFor="ds-method" hint={METHOD_HINT[f.method]}>
          <Select id="ds-method" value={f.method} onChange={(e) => changeMethod(e.target.value as TrainMethod)}>
            {METHODS.map((m) => <option key={m} value={m}>{METHOD_LABEL[m]}</option>)}
          </Select>
        </Field>
        <Field label="Nome" htmlFor="ds-name" hint="Facoltativo."><Input id="ds-name" value={f.name} onChange={(e) => set("name", e.target.value)} autoComplete="off" /></Field>
        {METHOD_SOURCES[f.method].length > 0 && (
          <fieldset className="tr-sources">
            <legend>{f.method === "dpo" ? "Da dove prendere le coppie" : "Da dove prendere gli esempi"}</legend>
            {METHOD_SOURCES[f.method].map((id) => (
              <label key={id}><input type="checkbox" checked={f.sources.includes(id)} onChange={() => toggle(id)} /> {(f.method === "dpo" ? DPO_SOURCE_LABEL : SOURCE_LABEL)[id]}</label>
            ))}
          </fieldset>
        )}
        {f.method === "grpo" && (
          <>
            <Field label="Suite" htmlFor="ds-suite" hint="I task danno i prompt e i loro grader la reward. Sono ammessi solo grader sulla risposta (output_matches, json_schema, judge).">
              <Select id="ds-suite" value={f.suite} onChange={(e) => set("suite", e.target.value)}>
                <option value="">Scegli…</option>
                {(suites.data ?? []).map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
              </Select>
            </Field>
            <Field label="Ripetizioni" htmlFor="ds-repeat" hint="Quante volte compare ogni prompt.">
              <Input id="ds-repeat" inputMode="numeric" value={f.repeat} onChange={(e) => set("repeat", e.target.value)} placeholder="1" />
            </Field>
          </>
        )}
        {f.sources.includes("eval") && (
          <Field label="Report di valutazione" htmlFor="ds-reports" hint="Gli id dei report, separati da virgola o spazio.">
            <Input id="ds-reports" value={f.evalReports} onChange={(e) => set("evalReports", e.target.value)} mono autoComplete="off" />
          </Field>
        )}
        <Field label="Agente" htmlFor="ds-agent" hint="Solo le sessioni di questo agente.">
          <Select id="ds-agent" value={f.agent} onChange={(e) => set("agent", e.target.value)}>
            <option value="">Tutti</option>
            {(agents.data ?? []).map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </Select>
        </Field>
        <div className="tr-form-grid">
          <Field label="Dal" htmlFor="ds-since"><Input id="ds-since" type="date" value={f.since} onChange={(e) => set("since", e.target.value)} /></Field>
          <Field label="Al" htmlFor="ds-until"><Input id="ds-until" type="date" value={f.until} onChange={(e) => set("until", e.target.value)} /></Field>
          <Field label="Al massimo" htmlFor="ds-limit"><Input id="ds-limit" inputMode="numeric" value={f.limit} onChange={(e) => set("limit", e.target.value)} placeholder="tutti" /></Field>
        </div>
        {(shown || build.isError) && <p className="tr-error" role="alert">{shown ?? errorMessage(build.error)}</p>}
        <button type="submit" className="sr-only" tabIndex={-1}>Crea</button>
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
    { key: "name", header: "Dataset", render: (d) => <><b>{d.name}</b><div className="faint text-xs mono">{d.id}</div></> },
    { key: "method", header: "Metodo", render: (d) => METHOD_LABEL[d.method], nowrap: true },
    { key: "n", header: "Esempi", className: "num", nowrap: true, render: (d) => <>{d.examples} <span className="faint text-xs">{d.train} + {d.valid}</span></> },
    { key: "tokens", header: "Token", className: "num", nowrap: true, render: (d) => `~${formatCompact(d.tokens_est)}` },
    { key: "from", header: "Origine", render: (d) => <span className="text-sm">{d.sources.map(sourceLabel).join(", ")}{d.agents.length ? ` · ${d.agents.join(", ")}` : ""}</span> },
    { key: "when", header: "Creato", nowrap: true, render: (d) => <span title={formatDateTime(d.created_at)}>{relTime(d.created_at)}</span> },
    { key: "rm", header: "", render: (d) => <Button size="sm" variant="ghost" icon="trash" aria-label={`Elimina ${d.name}`} onClick={() => setRemoving(d)} /> },
  ];
  return (
    <Section
      title="Dataset" hint="Esempi per l'addestramento: il 90% per imparare, il 10% per controllare."
      actions={<Button variant="primary" icon="plus" onClick={() => setBuilding(true)}>Nuovo dataset</Button>}
    >
      <QueryState
        query={datasets} isEmpty={(d) => d.length === 0}
        empty={<EmptyState icon="file" title="Nessun dataset" description="Dai un pollice in su alle risposte buone nelle conversazioni, poi crea un dataset: le sessioni approvate diventano esempi." action={<Button variant="primary" onClick={() => setBuilding(true)}>Crea il primo dataset</Button>} />}
      >
        {(all) => <DataTable label="Dataset di addestramento" columns={columns} rows={all} rowKey={(d) => d.id} />}
      </QueryState>
      {building && <BuildDialog onClose={() => setBuilding(false)} />}
      {removing && (
        <DecisionDialog
          title="Eliminare il dataset?" confirmLabel="Elimina" icon="trash" pending={del.isPending} error={del.error ?? undefined}
          onCancel={() => { setRemoving(null); del.reset(); }} onConfirm={() => del.mutate(removing.id, { onSuccess: () => setRemoving(null) })}
        >
          <p>«{removing.name}» ({removing.examples} esempi) viene cancellato dal disco. Gli adapter già addestrati non cambiano.</p>
        </DecisionDialog>
      )}
    </Section>
  );
}
