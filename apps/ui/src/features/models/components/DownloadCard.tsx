import { navigate } from "../../../app/router";
import { Button, Chip, Icon, Led, Meter, Pill } from "../../../shared/ui";
import { formatBytes, formatPercent, formatSpeed } from "../../../shared/lib/format";
import type { DownloadProgress } from "../../../shared/api/types";
import { useCancelDownload, useClearDownload, useRetryDownload } from "../api";
import { DOWNLOAD_STATUS, etaSeconds, filesLabel, formatEta, isActiveJob, jobName, progressRatio, type RetryPlan } from "../model/downloads";
import { InlineError } from "./HubError";

import { t } from "../../../shared/i18n";
export function DownloadStatusPill({ status }: { status: DownloadProgress["status"] }) {
  const s = DOWNLOAD_STATUS[status] ?? { label: status, tone: "neutral" as const, led: "off" as const };
  return (
    <Pill tone={s.tone}>
      <Led tone={s.led} small />
      {s.label}
    </Pill>
  );
}

/**
 * One job of the queue: status, aggregated progress over all files, speed and time left, the current file,
 * a persistent error with its next step, and the actions that make sense for its state.
 */
export function DownloadCard({ job: d, providerLabel, plan, onSetToken }: {
  job: DownloadProgress;
  providerLabel: string;
  /** What "Riprova" does for this job; null hides the button. */
  plan: RetryPlan;
  onSetToken: (providerId: string) => void;
}) {
  const cancel = useCancelDownload();
  const clear = useClearDownload();
  const retry = useRetryDownload();
  const active = isActiveJob(d);
  const ratio = progressRatio(d);
  const eta = etaSeconds(d);
  const files = filesLabel(d);
  const tone = d.status === "error" ? "crit" : d.status === "done" ? "ok" : undefined;
  const name = jobName(d);

  return (
    <article className="mh-job" data-status={d.status} aria-label={t("models.download_di", { name })}>
      <header className="mh-job-head">
        <Icon name="download" />
        <div className="mh-title">
          <h3 className="mono ellipsis" title={name}>{name}</h3>
          <span className="faint text-xs">{providerLabel}{d.variant ? <> · <Chip>{d.variant}</Chip></> : null}</span>
        </div>
        <DownloadStatusPill status={d.status} />
      </header>

      <Meter label={t("models.avanzamento_di_2", { name })} value={d.status === "done" ? 1 : d.received} max={d.status === "done" ? 1 : d.total} tone={tone} />
      <div className="mh-job-stats num">
        <span>{d.total > 0 ? formatPercent(ratio) : "—"}</span>
        <span>{formatBytes(d.received)} / {formatBytes(d.total)}</span>
        {files && <span>{files}</span>}
        {d.status === "running" && d.speed_bps > 0 && <span>{formatSpeed(d.speed_bps)}</span>}
        {eta !== null && <span>{t("models.mancano_2", { p: formatEta(eta) })}</span>}
        {d.status === "verifying" && <span>{t("models.controllo_l_integrita_dei_file")}</span>}
      </div>
      {active && d.file && <span className="mono faint text-xs ellipsis" title={d.file}>{d.file}</span>}
      {d.status === "done" && d.dest && <span className="mono faint text-xs ellipsis" title={d.dest}>{t("models.salvato_in", { dest: d.dest })}</span>}

      {d.error && <InlineError message={d.error} onSetToken={() => onSetToken(d.provider === "catalog" ? "hf" : d.provider)} />}
      {retry.isError && <InlineError error={retry.error} />}

      <div className="mh-job-actions">
        {active && <Button size="sm" onClick={() => cancel.mutate(d.id)} disabled={cancel.isPending}>{t("models.annulla")}</Button>}
        {plan?.kind === "pull" && (
          <Button size="sm" variant="primary" icon="refresh" onClick={() => retry.mutate({ job: d, plan })} disabled={retry.isPending}>
            {retry.isPending ? t("models.riavvio") : t("models.riprova")}
          </Button>
        )}
        {plan?.kind === "url" && (
          <Button size="sm" variant="primary" icon="refresh" title="Riapre il modulo con l'indirizzo già inserito" onClick={() => navigate(`/models/discover?provider=url&url=${encodeURIComponent(plan.url)}`)}>
            {t("models.riprova_dal_modulo")}</Button>
        )}
        {d.status === "done" && <Button size="sm" icon="cube" onClick={() => navigate("/models")}>{t("models.vedi_tra_gli_installati")}</Button>}
        {!active && <Button size="sm" variant="ghost" onClick={() => clear.mutate(d.id)} disabled={clear.isPending}>{t("models.rimuovi_dalla_lista")}</Button>}
      </div>
    </article>
  );
}
