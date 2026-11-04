import { useState } from "react";
import { Link } from "../../../app/router";
import { Icon, Input, StatusPill, Toggle } from "../../../shared/ui";
import { formatDateTime } from "../../../shared/lib/format";
import type { WorkflowDef, WorkflowRun } from "../../../shared/api/types";
import { summarizeTrigger } from "../model/triggers";

const SEARCH_FROM = 7;
const NEW = "new";

/** Workflows as links (the open one lives in the URL) with the on/off switch and when each one runs next. */
export function WorkflowList({ workflows, runs, selectedId, onToggle, togglingId }: {
  workflows: WorkflowDef[];
  runs: WorkflowRun[];
  selectedId: string | undefined;
  onToggle: (w: WorkflowDef, enabled: boolean) => void;
  togglingId: string | null;
}) {
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const shown = q ? workflows.filter((w) => `${w.name} ${w.description ?? ""} ${w.id}`.toLowerCase().includes(q)) : workflows;
  return (
    <div className="panel wf-list">
      {workflows.length >= SEARCH_FROM && (
        <div className="wf-search"><Input type="search" aria-label="Cerca un workflow" placeholder="Cerca un workflow…" value={query} onChange={(e) => setQuery(e.target.value)} /></div>
      )}
      <nav className="list" aria-label="Workflow">
        {selectedId === NEW && (
          <div className="list-item wf-item" aria-current="true">
            <Icon name="plus" />
            <div className="grow"><b>Nuovo workflow</b><div className="faint text-xs">Non ancora salvato</div></div>
          </div>
        )}
        {shown.map((w) => {
          const last = runs.find((r) => r.workflow_id === w.id);
          const t = summarizeTrigger(w.trigger, { enabled: w.enabled, lastRun: last });
          return (
            <div key={w.id} className="list-item wf-item" aria-current={w.id === selectedId}>
              <Link to={`/workflows/${encodeURIComponent(w.id)}`} className="list-link">
                <Icon name="flow" />
                <div className="grow">
                  <b>{w.name}</b>
                  <div className="faint text-xs">{t.label} · {w.steps.length} {w.steps.length === 1 ? "passo" : "passi"}</div>
                  {t.next && <div className="wf-next text-xs"><Icon name="refresh" size={12} /> prossima: {formatDateTime(t.next)}{t.note ? ` (${t.note})` : ""}</div>}
                  {!t.next && t.note && <div className="faint text-xs">{t.note}</div>}
                  {last && <div className="wf-last"><StatusPill status={last.status} /></div>}
                </div>
              </Link>
              <Toggle label={`${w.enabled ? "Disattiva" : "Attiva"} ${w.name}`} checked={w.enabled} disabled={togglingId === w.id} onChange={(on) => onToggle(w, on)} />
            </div>
          );
        })}
        {shown.length === 0 && <div className="list-item faint">Nessun workflow corrisponde.</div>}
      </nav>
    </div>
  );
}
