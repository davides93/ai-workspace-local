import { Button, EmptyState, Icon, Meter, Pill, StatusPill } from "../../../shared/ui";
import { formatBytes, formatPercent, formatSpeed } from "../../../shared/lib/format";
import type { DownloadProgress } from "../../../shared/api/types";
import { isActiveDownload, useCancelDownload, useClearDownload } from "../api";

function DownloadRow({ d }: { d: DownloadProgress }) {
  const cancel = useCancelDownload();
  const clear = useClearDownload();
  const active = isActiveDownload(d);
  return (
    <div className="list-item download-row">
      <Icon name="download" />
      <div className="grow stack stack-sm">
        <div className="row row-between">
          <span className="mono ellipsis text-sm">{d.model_ref || d.model_id}{d.variant ? ` · ${d.variant}` : ""}</span>
          <StatusPill status={d.status} />
        </div>
        <Meter label={`Avanzamento di ${d.model_ref || d.model_id}`} value={d.received} max={d.total} tone={d.status === "error" ? "crit" : d.status === "done" ? "ok" : undefined} />
        <div className="row row-between faint text-xs">
          <span className="mono ellipsis">{d.file}</span>
          <span className="num">
            {d.files_total > 1 && `file ${Math.min(d.files_done + (active ? 1 : 0), d.files_total)}/${d.files_total} · `}
            {formatBytes(d.received)} / {formatBytes(d.total)}
            {d.total > 0 && ` (${formatPercent(d.received / d.total)})`}
            {d.status === "running" && d.speed_bps > 0 && ` · ${formatSpeed(d.speed_bps)}`}
          </span>
        </div>
        {d.error && <span className="text-sm crit-text" role="alert">{d.error}</span>}
      </div>
      {active ? (
        <Button size="sm" onClick={() => cancel.mutate(d.id)} disabled={cancel.isPending}>Annulla</Button>
      ) : (
        <Button size="sm" variant="ghost" onClick={() => clear.mutate(d.id)} disabled={clear.isPending}>Rimuovi</Button>
      )}
    </div>
  );
}

/** Every download job (active, finished, failed or cancelled) with cancel and clear actions. */
export function DownloadsList({ jobs }: { jobs: DownloadProgress[] }) {
  if (jobs.length === 0) {
    return <EmptyState icon="download" title="Nessun download" description="I download avviati da Scopri o dal catalogo compaiono qui, con avanzamento ed eventuali errori." />;
  }
  return (
    <div className="panel">
      <div className="list">{jobs.map((d) => <DownloadRow key={d.id} d={d} />)}</div>
    </div>
  );
}

/** Compact strip above the tabs while something is downloading. */
export function ActiveDownloads({ jobs }: { jobs: DownloadProgress[] }) {
  const active = jobs.filter(isActiveDownload);
  if (active.length === 0) return null;
  return (
    <section className="section" aria-label="Download in corso">
      <div className="section-head"><h2>Download in corso</h2><Pill tone="run">{active.length}</Pill></div>
      <div className="panel">
        <div className="list">{active.map((d) => <DownloadRow key={d.id} d={d} />)}</div>
      </div>
    </section>
  );
}
