import { useState } from "react";
import type { UseQueryResult } from "@tanstack/react-query";
import { Link } from "../../../app/router";
import { Button, EmptyState, Meter, QueryState, Section, StatusPill } from "../../../shared/ui";
import { formatDateTime, relTime } from "../../../shared/lib/format";
import type { DatasetInfo, TrainDoctor, TrainJob } from "../../../shared/api/types";
import { useCancelTrainJob, useTrainLog } from "../api";
import { isActive, jobPercent, METHOD_LABEL, metricsLine } from "../model/training";
import { DoctorPanel } from "./DoctorPanel";
import { JobDialog } from "./JobDialog";

function JobLog({ job }: { job: TrainJob }) {
  const log = useTrainLog(job.id, isActive(job));
  const lines = log.data ?? [];
  return (
    <pre className="tr-log mono" aria-label="Log dell'addestramento" tabIndex={0}>
      {lines.length > 0 ? lines.join("\n") : log.isPending ? "Carico il log…" : "Il log è ancora vuoto."}
    </pre>
  );
}

function JobCard({ job }: { job: TrainJob }) {
  const [showLog, setShowLog] = useState(job.status === "failed");
  const cancel = useCancelTrainJob();
  const pct = jobPercent(job);
  const metrics = metricsLine(job.metrics);
  return (
    <li className="tr-job">
      <div className="tr-job-head">
        <b className="grow">{job.name ?? `${METHOD_LABEL[job.method]} · ${job.base_model}`}</b>
        <StatusPill status={job.status} />
      </div>
      <span className="faint text-sm">
        {METHOD_LABEL[job.method]} · {job.trainer} · {job.base_model} · <span title={formatDateTime(job.created_at)}>{relTime(job.created_at)}</span>
      </span>
      {isActive(job) && pct !== null && <Meter value={pct} max={100} label="Avanzamento" />}
      {isActive(job) && pct !== null && <span className="faint text-xs">{job.adapter_id ? "Confronto con il modello base in corso…" : `${pct}%`}</span>}
      {metrics && <span className="text-sm">{metrics}</span>}
      {job.error && <p className="tr-error" role="alert">{job.error}</p>}
      {job.metrics?.eval != null && <EvalLine eval={job.metrics.eval as Record<string, unknown>} />}
      <div className="tr-actions">
        <Button size="sm" onClick={() => setShowLog((v) => !v)} aria-expanded={showLog}>{showLog ? "Nascondi il log" : "Mostra il log"}</Button>
        {isActive(job) && <Button size="sm" variant="danger" icon="stop" onClick={() => cancel.mutate(job.id)} disabled={cancel.isPending}>Interrompi</Button>}
        {job.adapter_id && <Link to="/training/adapters" className="btn sm">Vai all'adapter</Link>}
      </div>
      {showLog && <JobLog job={job} />}
    </li>
  );
}

/** The comparison of the suite with and without the adapter, when the job asked for it. */
function EvalLine({ eval: e }: { eval: Record<string, unknown> }) {
  if (typeof e.error === "string") return <span className="faint text-sm">Confronto non riuscito: {e.error}</span>;
  const rate = (side: unknown) => {
    const s = side as { passed?: number; total?: number } | undefined;
    return s ? `${s.passed ?? 0}/${s.total ?? 0}` : "—";
  };
  return <span className="text-sm">Suite «{String(e.suite)}»: senza adapter {rate(e.baseline)}, con adapter {rate(e.adapter)}.</span>;
}

/** Device and trainers, the button that starts a fine-tuning and the list of the ones that ran. */
export function JobsTab({ doctor, jobs, datasets }: { doctor: UseQueryResult<TrainDoctor>; jobs: UseQueryResult<TrainJob[]>; datasets: DatasetInfo[] }) {
  const [starting, setStarting] = useState(false);
  const busy = (jobs.data ?? []).some(isActive);
  return (
    <div className="stack stack-lg">
      <details>
        <summary>Trainer e capacità di questo dispositivo</summary>
        <DoctorPanel doctor={doctor} />
      </details>
      <Section
        title="Addestramenti"
        hint={busy ? "Un addestramento alla volta: aspetta che finisca o interrompilo." : undefined}
        actions={<Button variant="primary" icon="bolt" onClick={() => setStarting(true)} disabled={busy}>Nuovo addestramento</Button>}
      >
        <QueryState
          query={jobs} isEmpty={(j) => j.length === 0}
          empty={<EmptyState icon="bolt" title="Nessun addestramento" description="Crea un dataset dalle conversazioni a cui hai dato un pollice in su, poi avvia l'addestramento: alla fine trovi l'adapter da usare in un profilo." action={<Button onClick={() => setStarting(true)}>Inizia con la guida</Button>} />}
        >
          {(all) => <ul className="tr-jobs">{all.map((j) => <JobCard key={j.id} job={j} />)}</ul>}
        </QueryState>
      </Section>
      {starting && <JobDialog doctor={doctor.data} datasets={datasets} onClose={() => setStarting(false)} />}
    </div>
  );
}
