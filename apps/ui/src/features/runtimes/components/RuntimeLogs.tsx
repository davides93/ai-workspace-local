import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { CopyButton, IconButton, Led, Toggle } from "../../../shared/ui";
import { LOGS_POLL_MS, useRuntimeLogs } from "../api";
import { logTail } from "../model/runtimes";

import { t } from "../../../shared/i18n";
/** Process log of a runtime. It refreshes by itself while open (switch it off to read in peace) and follows the end unless you scroll up. */
export function RuntimeLogs({ runtimeId }: { runtimeId: string }) {
  const [live, setLive] = useState(true);
  const logs = useRuntimeLogs(runtimeId, { poll: live });
  const lines = logTail(logs.data ?? []);
  const text = lines.join("\n");
  const box = useRef<HTMLPreElement>(null);
  const follow = useRef(true);

  useEffect(() => {
    const el = box.current;
    if (el && follow.current) el.scrollTop = el.scrollHeight;
  }, [text]);

  return (
    <section className="stack" aria-labelledby={`rt-logs-${runtimeId}`}>
      <div className="row row-between">
        <h3 id={`rt-logs-${runtimeId}`}>{t("runtimes.log_del_processo")}</h3>
        <div className="row nowrap">
          {live && <span className="faint text-xs row nowrap"><Led tone="run" small /> {t("runtimes.ogni_s", { seconds: LOGS_POLL_MS / 1000 })}</span>}
          <Toggle label={t("runtimes.aggiorna_i_log_da_solo")} checked={live} onChange={setLive} />
          <IconButton aria-label={t("runtimes.aggiorna_i_log_ora")} icon="refresh" size={16} onClick={() => void logs.refetch()} disabled={logs.isFetching} />
          {lines.length > 0 && <CopyButton text={text} label={t("runtimes.copia")} />}
        </div>
      </div>
      {logs.isError && <p className="crit-text text-sm" role="alert">{t("runtimes.impossibile_leggere_i_log", { error: errorMessage(logs.error) })}</p>}
      {lines.length > 0 ? (
        <pre
          ref={box} className="mono logbox" tabIndex={0} role="log" aria-label={t("runtimes.log_di", { runtimeId })} aria-live="off"
          onScroll={(e) => { const el = e.currentTarget; follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24; }}
        >{text}</pre>
      ) : (
        <p className="faint text-sm">{logs.isPending ? t("runtimes.leggo_i_log") : t("runtimes.nessun_log_il_processo_non")}</p>
      )}
    </section>
  );
}
