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

import { t } from "../../../shared/i18n";
function JobLog({ job }: { job: TrainJob }) {
  const log = useTrainLog(job.id, isActive(job));
  const lines = log.data ?? [];
  return (
    <pre className="tr-log mono" aria-label={t("training.log_dell_addestramento")} tabIndex={0}>
      {lines.length > 0 ? lines.join("\n") : log.isPending ? t("training.carico_il_log") : t("training.il_log_e_ancora_vuoto")}
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
      {isActive(job) && pct !== null && <Meter value={pct} max={100} label={t("training.avanzamento")} />}
      {isActive(job) && pct !== null && <span className="faint text-xs">{job.adapter_id ? t("training.confronto_con_il_modello_base") : `${pct}%`}</span>}
      {metrics && <span className="text-sm">{metrics}</span>}
      {job.error && <p className="tr-error" role="alert">{job.error}</p>}
      {job.metrics?.eval != null && <EvalLine eval={job.metrics.eval as Record<string, unknown>} />}
      <div className="tr-actions">
        <Button size="sm" onClick={() => setShowLog((v) => !v)} aria-expanded={showLog}>{showLog ? t("training.nascondi_il_log") : t("training.mostra_il_log")}</Button>
        {isActive(job) && <Button size="sm" variant="danger" icon="stop" onClick={() => cancel.mutate(job.id)} disabled={cancel.isPending}>{t("training.interrompi")}</Button>}
        {job.adapter_id && <Link to="/training/adapters" className="btn sm">{t("training.vai_all_adapter")}</Link>}
      </div>
      {showLog && <JobLog job={job} />}
    </li>
  );
}

/** The comparison of the suite with and without the adapter, when the job asked for it. */
function EvalLine({ eval: e }: { eval: Record<string, unknown> }) {
  if (typeof e.error === "string") return <span className="faint text-sm">{t("training.confronto_non_riuscito", { error: e.error })}</span>;
  const rate = (side: unknown) => {
    const s = side as { passed?: number; total?: number } | undefined;
    return s ? `${s.passed ?? 0}/${s.total ?? 0}` : "—";
  };
  return <span className="text-sm">{t("training.suite_senza_adapter_con_adapter", { suite: String(e.suite), baseline: rate(e.baseline), adapter: rate(e.adapter) })}</span>;
}

/** Device and trainers, the button that starts a fine-tuning and the list of the ones that ran. */
export function JobsTab({ doctor, jobs, datasets }: { doctor: UseQueryResult<TrainDoctor>; jobs: UseQueryResult<TrainJob[]>; datasets: DatasetInfo[] }) {
  const [starting, setStarting] = useState(false);
  const busy = (jobs.data ?? []).some(isActive);
  return (
    <div className="stack stack-lg">
      <details>
        <summary>{t("training.trainer_e_capacita_di_questo")}</summary>
        <DoctorPanel doctor={doctor} />
      </details>
      <Section
        title={t("training.addestramenti")}
        hint={busy ? t("training.un_addestramento_alla_volta_aspetta") : undefined}
        actions={<Button variant="primary" icon="bolt" onClick={() => setStarting(true)} disabled={busy}>{t("training.nuovo_addestramento_2")}</Button>}
      >
        <QueryState
          query={jobs} isEmpty={(j) => j.length === 0}
          empty={<EmptyState icon="bolt" title={t("training.nessun_addestramento")} description={t("training.crea_un_dataset_dalle_conversazioni")} action={<Button onClick={() => setStarting(true)}>{t("training.inizia_con_la_guida")}</Button>} />}
        >
          {(all) => <ul className="tr-jobs">{all.map((j) => <JobCard key={j.id} job={j} />)}</ul>}
        </QueryState>
      </Section>
      {starting && <JobDialog doctor={doctor.data} datasets={datasets} onClose={() => setStarting(false)} />}
    </div>
  );
}
