import { useMemo } from "react";
import { navigate } from "../../../app/router";
import { Button, EmptyState, Panel } from "../../../shared/ui";
import { formatBytes, formatSpeed } from "../../../shared/lib/format";
import type { DownloadProgress } from "../../../shared/api/types";
import { useCatalog, useClearFinishedDownloads, useModelProviders } from "../api";
import { formatEta, retryPlan, sortJobs, summarizeQueue } from "../model/downloads";
import { providerName } from "../model/providers";
import { Stat, Stats } from "./common";
import { DownloadCard } from "./DownloadCard";
import { useProviderAuthDialog } from "./ProviderAuthDialog";

import { t } from "../../../shared/i18n";
/** The queue: every job with progress, speed, time left, persistent errors, retry, cancel and clear. */
export function DownloadsTab({ jobs }: { jobs: DownloadProgress[] }) {
  const catalog = useCatalog();
  const providers = useModelProviders();
  const clearAll = useClearFinishedDownloads();
  const auth = useProviderAuthDialog();
  const catalogIds = useMemo(() => new Set((catalog.data ?? []).map((c) => c.id)), [catalog.data]);
  const sorted = useMemo(() => sortJobs(jobs), [jobs]);
  const summary = useMemo(() => summarizeQueue(jobs), [jobs]);
  const finishedIds = jobs.filter((d) => d.status === "done" || d.status === "cancelled").map((d) => d.id);

  if (jobs.length === 0) {
    return (
      <EmptyState
        headingLevel={2} icon="download" title={t("models.nessun_download")}
        description={t("models.i_download_avviati_da_scopri")}
        action={<><Button variant="primary" icon="search" onClick={() => navigate("/models/discover")}>{t("models.scopri_modelli")}</Button><Button onClick={() => navigate("/models/catalog")}>{t("models.apri_il_catalogo")}</Button></>}
      />
    );
  }

  return (
    <div className="stack stack-lg">
      <h2 className="sr-only">{t("models.coda_dei_download")}</h2>
      {summary.active > 0 && (
        <Panel title={t("models.coda_in_corso")} icon="activity">
          <Stats wide>
            <Stat label={t("models.attivi")}>{summary.running + summary.verifying}{summary.queued > 0 ? t("models.in_coda_2", { queued: summary.queued }) : ""}</Stat>
            <Stat label={t("models.velocita")}>{formatSpeed(summary.speedBps)}</Stat>
            <Stat label={t("models.da_scaricare")}>{formatBytes(summary.remainingBytes)}</Stat>
            <Stat label={t("models.tempo_stimato")}>{summary.etaSeconds === null ? "—" : formatEta(summary.etaSeconds)}</Stat>
          </Stats>
        </Panel>
      )}
      {finishedIds.length > 0 && (
        <div className="row row-between">
          <span className="faint text-sm">{summary.failed > 0 ? t("models.gli_errori_restano_qui_finche") : t("models.download_conclusi")}</span>
          <Button size="sm" variant="ghost" icon="trash" onClick={() => clearAll.mutate(finishedIds)} disabled={clearAll.isPending}>
            {t("models.rimuovi_i_conclusi", { length: finishedIds.length })}</Button>
        </div>
      )}
      <div className="mh-jobs">
        {sorted.map((d) => (
          <DownloadCard
            key={d.id} job={d} plan={retryPlan(d, catalogIds)} onSetToken={auth.open}
            providerLabel={providerName(d.provider || "hf", providers.data)}
          />
        ))}
      </div>
      {auth.dialog}
    </div>
  );
}
