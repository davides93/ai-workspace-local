import { useMemo, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useApi } from "../../../shared/api/context";
import { errorMessage } from "../../../shared/lib/errors";
import { qk } from "../../../shared/query/keys";
import { Icon, Led } from "../../../shared/ui";
import type { Item } from "../model/items";
import { traceItems } from "../model/trace";

import { t } from "../../../shared/i18n";
type SubagentItem = Extract<Item, { kind: "subagent" }>;

/**
 * A sub-agent (or team member) in the conversation. When the backend named its run, the card opens to
 * its own trace: text, tool calls and results, loaded from the run's events (refreshed while it works).
 * `renderItem` draws one row of the trace, so that the card does not depend on the row view.
 */
export function SubagentCard({ it, renderItem }: { it: SubagentItem; renderItem: (row: Item) => ReactNode }) {
  const api = useApi();
  const [open, setOpen] = useState(false);
  const running = it.result === undefined && !it.interrupted;
  const runId = it.runId;
  const trace = useQuery({
    queryKey: qk.runEvents(runId ?? ""),
    queryFn: () => api.runEvents(runId!),
    enabled: open && !!runId,
    refetchInterval: running ? 1500 : false,
  });
  const rows = useMemo(() => (trace.data ? traceItems(trace.data) : []), [trace.data]);
  const bodyId = `sub-${it.id}`;
  return (
    <div className="ws-subagent-wrap">
      <div className="ws-subagent">
        <Icon name="agent" size={15} />
        <span>Subagente <b>{it.name}</b></span>
        {it.result !== undefined ? <span className="muted">→ {it.result}</span> : it.interrupted ? <span className="muted">{t("chat.interrotto")}</span> : <Led tone="run" />}
        {runId && (
          <button type="button" className="ws-subagent-toggle" aria-expanded={open} aria-controls={bodyId} onClick={() => setOpen((o) => !o)}>
            <Icon name="down" size={13} className={open ? undefined : "ws-subagent-chev"} /> {open ? t("chat.nascondi_traccia") : t("chat.mostra_traccia")}
          </button>
        )}
      </div>
      {open && (
        <div id={bodyId} className="ws-subagent-trace" role="region" aria-label={t("chat.traccia_di", { name: it.name })}>
          {trace.isPending && <span className="faint text-sm">{t("chat.carico_la_traccia")}</span>}
          {trace.isError && <span className="faint text-sm">{t("chat.traccia_non_disponibile", { p: errorMessage(trace.error) })}</span>}
          {trace.data && rows.length === 0 && <span className="faint text-sm">{t("chat.il_subagente_non_ha_ancora")}</span>}
          {rows.map((row, i) => <div key={`${row.kind}-${"id" in row ? row.id : i}-${i}`}>{renderItem(row)}</div>)}
        </div>
      )}
    </div>
  );
}
