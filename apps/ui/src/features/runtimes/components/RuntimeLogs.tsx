import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { CopyButton, IconButton, Led, Toggle } from "../../../shared/ui";
import { LOGS_POLL_MS, useRuntimeLogs } from "../api";
import { logTail } from "../model/runtimes";

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
        <h3 id={`rt-logs-${runtimeId}`}>Log del processo</h3>
        <div className="row nowrap">
          {live && <span className="faint text-xs row nowrap"><Led tone="run" small /> ogni {LOGS_POLL_MS / 1000} s</span>}
          <Toggle label="Aggiorna i log da solo" checked={live} onChange={setLive} />
          <IconButton aria-label="Aggiorna i log ora" icon="refresh" size={16} onClick={() => void logs.refetch()} disabled={logs.isFetching} />
          {lines.length > 0 && <CopyButton text={text} label="Copia" />}
        </div>
      </div>
      {logs.isError && <p className="crit-text text-sm" role="alert">Impossibile leggere i log: {errorMessage(logs.error)}</p>}
      {lines.length > 0 ? (
        <pre
          ref={box} className="mono logbox" tabIndex={0} role="log" aria-label={`Log di ${runtimeId}`} aria-live="off"
          onScroll={(e) => { const el = e.currentTarget; follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24; }}
        >{text}</pre>
      ) : (
        <p className="faint text-sm">{logs.isPending ? "Leggo i log…" : "Nessun log: il processo non è stato avviato da questa app o non ha ancora scritto nulla."}</p>
      )}
    </section>
  );
}
