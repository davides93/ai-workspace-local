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
        icon="download" title="Nessun download"
        description="I download avviati da Scopri o dal catalogo compaiono qui, con avanzamento, velocità, tempo stimato ed eventuali errori."
        action={<><Button variant="primary" icon="search" onClick={() => navigate("/models/discover")}>Scopri modelli</Button><Button onClick={() => navigate("/models/catalog")}>Apri il catalogo</Button></>}
      />
    );
  }

  return (
    <div className="stack stack-lg">
      <h2 className="sr-only">Coda dei download</h2>
      {summary.active > 0 && (
        <Panel title="Coda in corso" icon="activity">
          <Stats wide>
            <Stat label="Attivi">{summary.running + summary.verifying}{summary.queued > 0 ? ` · ${summary.queued} in coda` : ""}</Stat>
            <Stat label="Velocità">{formatSpeed(summary.speedBps)}</Stat>
            <Stat label="Da scaricare">{formatBytes(summary.remainingBytes)}</Stat>
            <Stat label="Tempo stimato">{summary.etaSeconds === null ? "—" : formatEta(summary.etaSeconds)}</Stat>
          </Stats>
        </Panel>
      )}
      {finishedIds.length > 0 && (
        <div className="row row-between">
          <span className="faint text-sm">{summary.failed > 0 ? "Gli errori restano qui finché non li rimuovi." : "Download conclusi."}</span>
          <Button size="sm" variant="ghost" icon="trash" onClick={() => clearAll.mutate(finishedIds)} disabled={clearAll.isPending}>
            Rimuovi i conclusi ({finishedIds.length})
          </Button>
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
