import { Link } from "../../../app/router";
import { formatPercent, formatSpeed } from "../../../shared/lib/format";
import { Meter, Panel, QueryState, StatusPill } from "../../../shared/ui";
import { isActiveDownload, useDownloads } from "../../models/api";
import { downloadFraction } from "../model/overview";

import { t } from "../../../shared/i18n";
/** Downloads that are queued, transferring or being verified. */
export function DownloadsPanel() {
  const downloads = useDownloads();
  const active = (downloads.data ?? []).filter(isActiveDownload);
  return (
    <Panel title={t("home.download_in_corso")} icon="download" actions={<Link to="/models/downloads" className="btn sm ghost">{t("home.tutti")}</Link>}>
      <QueryState query={downloads}>
        {() =>
          active.length === 0 ? (
            <p className="muted text-sm">{t("home.nessun_download_attivo")}</p>
          ) : (
            <div className="stack stack-lg">
              {active.map((j) => (
                <div key={j.id} className="stack stack-sm">
                  <div className="row row-between nowrap">
                    <span className="mono ellipsis text-sm">{j.model_ref || j.model_id}</span>
                    <StatusPill status={j.status} />
                  </div>
                  <Meter label={t("home.avanzamento_di", { p: j.model_ref || j.model_id })} value={j.received} max={j.total} />
                  <span className="faint text-xs num">{formatPercent(downloadFraction(j.received, j.total))}{j.speed_bps > 0 ? ` · ${formatSpeed(j.speed_bps)}` : ""}</span>
                </div>
              ))}
            </div>
          )
        }
      </QueryState>
    </Panel>
  );
}
