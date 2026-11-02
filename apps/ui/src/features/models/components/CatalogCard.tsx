import { Link } from "../../../app/router";
import { Button, Chip, Icon, Meter, Pill } from "../../../shared/ui";
import { formatContext, formatNumber, formatParams, formatPercent, formatSpeed } from "../../../shared/lib/format";
import { toast } from "../../../shared/lib/toast";
import type { CatalogModel, DownloadProgress } from "../../../shared/api/types";
import { useCancelDownload, usePullModel } from "../api";
import { catalogSize, catalogState, catalogTags, platformNote, systemNote, systemRuntime } from "../model/catalog";
import { etaSeconds, formatEta, progressRatio } from "../model/downloads";
import { ChipRow, FitPill, RuntimeChips, Stat, Stats } from "./common";
import { InlineError } from "./HubError";

/** Curated entry: what it is, how it fits this device, who can run it, and the one-click download (or why there is none). */
export function CatalogCard({ model: c, job, onSetToken }: { model: CatalogModel; job?: DownloadProgress; onSetToken: () => void }) {
  const pull = usePullModel({ handled: true });
  const cancel = useCancelDownload();
  const state = catalogState(c, job);
  const runtime = systemRuntime(c);
  const note = platformNote(c);
  const eta = job ? etaSeconds(job) : null;

  return (
    <article className="mh-card" data-fit={c.downloadable ? c.fit || undefined : undefined} aria-label={c.name}>
      <header className="mh-card-head">
        <div className="mh-title">
          <h3 className="ellipsis" title={c.name}>{c.name}</h3>
          <span className="faint text-xs ellipsis">{c.publisher} · {c.license}</span>
        </div>
        {state === "system" ? <Pill tone="info" title="Fornito dal sistema operativo">Di sistema</Pill> : <FitPill fit={c.fit} detail={c.fit_detail} />}
      </header>

      <p className="mh-desc">{c.description}</p>

      <Stats>
        <Stat label="Parametri">
          {formatParams(c.params_b)}
          {c.active_params_b ? <small>{formatNumber(c.active_params_b)}B attivi</small> : null}
        </Stat>
        <Stat label="Contesto">{formatContext(c.context_length)}</Stat>
        <Stat label="Download">{catalogSize(c) ?? "di sistema"}</Stat>
      </Stats>

      <ChipRow label="Formato e capacità">
        {catalogTags(c).map((t) => <Chip key={t}>{t}</Chip>)}
        {c.capabilities.map((x) => <Chip key={x}>{x}</Chip>)}
      </ChipRow>
      {state !== "system" && c.fit_detail && <p className="faint text-xs"><b>Su questo device:</b> {c.fit_detail}</p>}
      {note && <p className="mh-note">{note}</p>}
      {state === "system" ? <p className="mh-note">{systemNote(runtime)}</p> : <RuntimeChips runtimes={c.runtime_compat} explain />}

      <footer className="mh-card-foot">
        {state === "system" && runtime && <Link to={`/runtimes/${runtime}`} className="btn sm">Vedi il runtime <Icon name="forward" size={13} /></Link>}
        {state === "installed" && <Pill tone="ok"><Icon name="check" size={13} /> Installato</Pill>}
        {state === "downloading" && job && (
          <div className="grow stack stack-sm">
            <Meter label={`Download di ${c.name}`} value={job.received} max={job.total} />
            <div className="row row-between text-xs faint num">
              <span>
                {job.status === "queued" ? "In coda" : job.status === "verifying" ? "Verifica sha256…" : formatPercent(progressRatio(job))}
                {job.status === "running" && job.speed_bps > 0 && ` · ${formatSpeed(job.speed_bps)}`}
                {eta !== null && ` · mancano ${formatEta(eta)}`}
              </span>
              <Button size="sm" variant="ghost" onClick={() => cancel.mutate(job.id)} disabled={cancel.isPending}>Annulla</Button>
            </div>
          </div>
        )}
        {(state === "ready" || state === "blocked") && (
          <Button
            variant="primary" icon="download" disabled={state === "blocked" || pull.isPending}
            title={state === "blocked" ? c.fit_detail : undefined}
            onClick={() => pull.mutate(c.id, { onSuccess: () => toast(`Download avviato: ${c.name}`) })}
          >
            {pull.isPending ? "Avvio…" : state === "blocked" ? "Non compatibile" : `Scarica ${catalogSize(c) ?? ""}`.trim()}
          </Button>
        )}
        {c.source.kind === "hf" && <span className="mono faint ellipsis text-xs ml-auto mh-repo" title={c.source.repo}>{c.source.repo}</span>}
      </footer>
      {pull.isError && <InlineError error={pull.error} onSetToken={onSetToken} />}
    </article>
  );
}
