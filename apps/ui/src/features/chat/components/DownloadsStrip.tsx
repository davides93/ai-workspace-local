import { formatPercent } from "../../../shared/lib/format";
import { Meter } from "../../../shared/ui";
import { downloadFraction } from "../../home/model/overview";
import { isActiveDownload, useDownloads } from "../../models/api";

import { t } from "../../../shared/i18n";
/**
 * The downloads that are going on, in a compact strip above the message box (the simple interface has no Modelli page to watch them
 * in). Nothing when there are none. No live region: the meter has its value, and a percentage read out at every step would drown the chat.
 */
export function DownloadsStrip() {
  const active = (useDownloads().data ?? []).filter(isActiveDownload);
  if (active.length === 0) return null;
  return (
    <section className="ws-downloads" aria-label={t("chat.download_in_corso")}>
      <ul>
        {active.map((j) => {
          const name = j.model_ref || j.model_id;
          return (
            <li key={j.id}>
              <span className="ws-dl-name">{t("chat.sto_scaricando", { name })}</span>
              <span className="ws-dl-pct num">{formatPercent(downloadFraction(j.received, j.total))}</span>
              <Meter label={t("chat.download_di", { name })} value={j.received} max={j.total} />
            </li>
          );
        })}
      </ul>
    </section>
  );
}
