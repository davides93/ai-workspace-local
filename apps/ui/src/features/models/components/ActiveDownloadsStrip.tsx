import { Link } from "../../../app/router";
import { Icon, Led, Meter } from "../../../shared/ui";
import { formatPercent, formatSpeed } from "../../../shared/lib/format";
import type { DownloadProgress } from "../../../shared/api/types";
import { etaSeconds, formatEta, isActiveJob, jobName, progressRatio } from "../model/downloads";

import { t } from "../../../shared/i18n";
/** Compact "in progress" strip above the tabs (not shown on the Download tab itself). */
export function ActiveDownloadsStrip({ jobs }: { jobs: DownloadProgress[] }) {
  const active = jobs.filter(isActiveJob);
  if (active.length === 0) return null;
  return (
    <section className="mh-strip" aria-label={t("models.download_in_corso")}>
      <div className="mh-strip-head">
        <Icon name="download" size={16} />
        <h2>In download <span className="faint">({active.length})</span></h2>
        <Link to="/models/downloads" className="link-more ml-auto text-sm">{t("models.gestisci")}{" "}<Icon name="forward" size={13} /></Link>
      </div>
      <ul className="mh-strip-list">
        {active.map((d) => {
          const eta = etaSeconds(d);
          return (
            <li key={d.id}>
              <Led tone={d.status === "queued" ? "off" : "run"} small />
              <span className="mono ellipsis" title={jobName(d)}>{jobName(d)}{d.variant ? ` · ${d.variant}` : ""}</span>
              <Meter label={t("models.avanzamento_di", { p: jobName(d) })} value={d.received} max={d.total} />
              <span className="num faint text-xs mh-strip-stats">
                {d.status === "queued" ? t("models.in_coda") : d.status === "verifying" ? t("models.verifica") : formatPercent(progressRatio(d))}
                {d.status === "running" && d.speed_bps > 0 && ` · ${formatSpeed(d.speed_bps)}`}
                {eta !== null && ` · ${formatEta(eta)}`}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
