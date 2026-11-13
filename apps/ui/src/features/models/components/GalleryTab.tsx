import { useMemo, useState } from "react";
import { navigate } from "../../../app/router";
import { Button, EmptyState, Icon, Meter, Pill, QueryState, Skeleton } from "../../../shared/ui";
import { formatContext, formatParams, formatPercent, formatSpeed } from "../../../shared/lib/format";
import { toast } from "../../../shared/lib/toast";
import type { CatalogModel, DownloadProgress, LocalModel } from "../../../shared/api/types";
import { useCancelDownload, useCatalog, useModels, usePullModel } from "../api";
import { catalogSize, catalogState, findCatalogJob } from "../model/catalog";
import { etaSeconds, formatEta, progressRatio } from "../model/downloads";
import { findInstalled, findSystem, galleryModels } from "../model/gallery";
import { FitPill } from "./common";
import { InlineError } from "./HubError";
import { SystemPreview } from "./InstalledCard";
import { UseInProfileDialog } from "./UseInProfileDialog";

import { t } from "../../../shared/i18n";
/** Galleria: the models that run on this device, as big cards — download, try, use. */
export function GalleryTab({ jobs }: { jobs: DownloadProgress[] }) {
  const catalog = useCatalog();
  const models = useModels();
  return (
    <QueryState
      query={catalog}
      skeleton={<div className="mh-grid">{Array.from({ length: 4 }, (_, i) => <Skeleton key={i} height={220} className="skeleton-card" />)}</div>}
    >
      {(list) => <Gallery list={list} jobs={jobs} installed={models.data ?? []} />}
    </QueryState>
  );
}

function Gallery({ list, jobs, installed }: { list: CatalogModel[]; jobs: DownloadProgress[]; installed: LocalModel[] }) {
  const shown = useMemo(() => galleryModels(list), [list]);
  const [use, setUse] = useState<LocalModel | null>(null);
  return (
    <div className="stack stack-lg">
      <h2 className="sr-only">{t("models.galleria_dei_modelli")}</h2>
      <p className="muted text-sm">
        {t("models.modelli_che_girano_sul_dispositivo")}</p>
      {shown.length === 0 ? (
        <EmptyState icon="cube" title={t("models.nessun_modello_in_galleria")} description={t("models.apri_il_catalogo_per_vedere")} action={<Button onClick={() => navigate("/models/catalog")}>{t("models.apri_il_catalogo")}</Button>} />
      ) : (
        <div className="mh-grid mh-gallery">
          {shown.map((c) => <GalleryCard key={c.id} model={c} job={findCatalogJob(jobs, c)} local={findInstalled(c, installed) ?? findSystem(c, installed)} onUse={setUse} />)}
        </div>
      )}
      {use && <UseInProfileDialog model={use} onClose={() => setUse(null)} />}
    </div>
  );
}

function GalleryCard({ model: c, job, local, onUse }: { model: CatalogModel; job?: DownloadProgress; local?: LocalModel; onUse: (m: LocalModel) => void }) {
  const pull = usePullModel({ handled: true });
  const cancel = useCancelDownload();
  const [preview, setPreview] = useState(false);
  const state = catalogState(c, job);
  const eta = job ? etaSeconds(job) : null;
  const system = state === "system";
  return (
    <article className="mh-card mh-gallery-card" data-fit={!system ? c.fit || undefined : undefined} aria-label={c.name}>
      <header className="mh-card-head">
        <div className="mh-title">
          <h3 className="ellipsis" title={c.name}>{c.name}</h3>
          <span className="faint text-xs ellipsis">{c.publisher} · {formatParams(c.params_b)} · {formatContext(c.context_length)}</span>
        </div>
        {system ? <Pill tone="info" title="Fornito dal sistema operativo">{t("models.di_sistema")}</Pill> : <FitPill fit={c.fit} detail={c.fit_detail} />}
      </header>
      <p className="mh-desc">{c.description}</p>
      {!system && c.fit_detail && <p className="faint text-xs">{c.fit_detail}</p>}

      <footer className="mh-card-foot">
        {state === "downloading" && job ? (
          <div className="grow stack stack-sm">
            <Meter label={t("models.download_di", { name: c.name })} value={job.received} max={job.total} />
            <div className="row row-between text-xs faint num">
              <span>
                {job.status === "queued" ? t("models.in_coda") : job.status === "verifying" ? t("models.verifica") : formatPercent(progressRatio(job))}
                {job.status === "running" && job.speed_bps > 0 && ` · ${formatSpeed(job.speed_bps)}`}
                {eta !== null && t("models.mancano", { p: formatEta(eta) })}
              </span>
              <Button size="sm" variant="ghost" onClick={() => cancel.mutate(job.id)} disabled={cancel.isPending}>{t("models.annulla")}</Button>
            </div>
          </div>
        ) : (
          <>
            {(state === "ready" || state === "blocked") && (
              <Button
                variant="primary" icon="download" disabled={state === "blocked" || pull.isPending} title={state === "blocked" ? c.fit_detail : undefined}
                onClick={() => pull.mutate(c.id, { onSuccess: () => toast(t("models.download_avviato", { name: c.name })) })}
              >
                {pull.isPending ? t("models.avvio") : state === "blocked" ? t("models.non_compatibile") : `Scarica ${catalogSize(c) ?? ""}`.trim()}
              </Button>
            )}
            {local && <Button variant="primary" onClick={() => setPreview(true)}>{t("models.prova")}</Button>}
            {local && <Button icon="link" onClick={() => onUse(local)}>{t("models.usa_nel_profilo")}</Button>}
            {state === "installed" && !local && <Pill tone="ok"><Icon name="check" size={13} /> {" "}{t("models.installato")}</Pill>}
            {system && !local && <span className="faint text-xs">{t("models.non_disponibile_su_questo_dispositivo")}</span>}
          </>
        )}
      </footer>
      {preview && local && <SystemPreview model={local} onClose={() => setPreview(false)} />}
      {pull.isError && <InlineError error={pull.error} onSetToken={() => undefined} />}
    </article>
  );
}
