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

import { t as tr, t } from "../../../shared/i18n";
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
        {state === "system" ? <Pill tone="info" title={t("models.fornito_dal_sistema_operativo")}>{tr("models.di_sistema")}</Pill> : <FitPill fit={c.fit} detail={c.fit_detail} />}
      </header>

      <p className="mh-desc">{c.description}</p>

      <Stats>
        <Stat label={tr("models.parametri")}>
          {formatParams(c.params_b)}
          {c.active_params_b ? <small>{tr("models.b_attivi", { p: formatNumber(c.active_params_b) })}</small> : null}
        </Stat>
        <Stat label={tr("models.contesto")}>{formatContext(c.context_length)}</Stat>
        <Stat label={tr("models.download")}>{catalogSize(c) ?? tr("models.di_sistema_2")}</Stat>
      </Stats>

      <ChipRow label={tr("models.formato_e_capacita")}>
        {catalogTags(c).map((t) => <Chip key={t}>{t}</Chip>)}
        {c.capabilities.map((x) => <Chip key={x}>{x}</Chip>)}
      </ChipRow>
      {state !== "system" && c.fit_detail && <p className="faint text-xs"><b>{tr("models.su_questo_device")}</b> {c.fit_detail}</p>}
      {note && <p className="mh-note">{note}</p>}
      {state === "system" ? <p className="mh-note">{systemNote(runtime)}</p> : <RuntimeChips runtimes={c.runtime_compat} explain />}

      <footer className="mh-card-foot">
        {state === "system" && runtime && <Link to={`/runtimes/${runtime}`} className="btn sm">{tr("models.vedi_il_runtime")}{" "}<Icon name="forward" size={13} /></Link>}
        {state === "installed" && <Pill tone="ok"><Icon name="check" size={13} /> {tr("models.installato")}</Pill>}
        {state === "downloading" && job && (
          <div className="grow stack stack-sm">
            <Meter label={tr("models.download_di", { name: c.name })} value={job.received} max={job.total} />
            <div className="row row-between text-xs faint num">
              <span>
                {job.status === "queued" ? tr("models.in_coda") : job.status === "verifying" ? tr("models.verifica_sha256") : formatPercent(progressRatio(job))}
                {job.status === "running" && job.speed_bps > 0 && ` · ${formatSpeed(job.speed_bps)}`}
                {eta !== null && tr("models.mancano", { p: formatEta(eta) })}
              </span>
              <Button size="sm" variant="ghost" onClick={() => cancel.mutate(job.id)} disabled={cancel.isPending}>{tr("models.annulla")}</Button>
            </div>
          </div>
        )}
        {(state === "ready" || state === "blocked") && (
          <Button
            variant="primary" icon="download" disabled={state === "blocked" || pull.isPending}
            title={state === "blocked" ? c.fit_detail : undefined}
            onClick={() => pull.mutate(c.id, { onSuccess: () => toast(tr("models.download_avviato", { name: c.name })) })}
          >
            {pull.isPending ? tr("models.avvio") : state === "blocked" ? tr("models.non_compatibile") : `Scarica ${catalogSize(c) ?? ""}`.trim()}
          </Button>
        )}
        {c.source.kind === "hf" && <span className="mono faint ellipsis text-xs ml-auto mh-repo" title={c.source.repo}>{c.source.repo}</span>}
      </footer>
      {pull.isError && <InlineError error={pull.error} onSetToken={onSetToken} />}
    </article>
  );
}
