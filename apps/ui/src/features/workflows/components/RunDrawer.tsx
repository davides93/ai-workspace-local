import { CopyButton, Drawer, Icon, KeyValue, Led, StatusPill } from "../../../shared/ui";
import { formatDateTime, formatDuration } from "../../../shared/lib/format";
import type { WorkflowDef, WorkflowRun } from "../../../shared/api/types";
import { stepMeta } from "../model/steps";
import { isActiveRun } from "../api";
import { runSeconds } from "../model/triggers";
import { runProgress } from "./RunsPanel";

/** Detail of one run: every step with its status, result or error. Updates by itself while the run goes on. */
export function RunDrawer({ run, workflow, onClose }: { run: WorkflowRun; workflow?: WorkflowDef; onClose: () => void }) {
  const active = isActiveRun(run);
  const secs = runSeconds(run.started_at, run.finished_at);
  const p = runProgress(run);
  return (
    <Drawer title="Esecuzione" subtitle={`${workflow?.name ?? run.workflow_id} · ${run.id}`} onClose={onClose}>
      <div className="run-summary">
        <KeyValue items={[
          ["Stato", <StatusPill key="s" status={run.status} />],
          ["Avviata", formatDateTime(run.started_at)],
          run.finished_at ? ["Conclusa", formatDateTime(run.finished_at)] : false,
          secs !== null && ["Durata", `${formatDuration(secs)}${active ? " finora" : ""}`],
          ["Passi", `${p.done} di ${p.total} completati`],
        ]} />
        {active && (
          <p className="row text-sm" role="status">
            <Led tone="run" small />
            {run.status === "waiting_approval" ? "In attesa della tua conferma: rispondi dal riquadro delle conferme in alto." : "In corso: la pagina si aggiorna da sola."}
          </p>
        )}
      </div>
      <ol className="run-steps-list" aria-label="Passi dell'esecuzione">
        {run.steps.map((s, i) => {
          const def = workflow?.steps.find((x) => x.id === s.id);
          return (
            <li key={s.id} className="run-step" data-status={s.status}>
              <div className="run-step-head">
                <span className="run-step-n" aria-hidden="true">{i + 1}</span>
                {def && <Icon name={stepMeta(def.kind).icon} size={15} />}
                <b className="mono text-sm">{s.id}</b>
                {def && <span className="faint text-xs">{stepMeta(def.kind).label}</span>}
                <span className="ml-auto"><StatusPill status={s.status} /></span>
              </div>
              {s.error && <p className="run-error" role="alert"><Icon name="alert" size={14} /> {s.error}</p>}
              {s.output && (
                <div className="run-output">
                  <div className="row row-between"><span className="eyebrow">Risultato</span><CopyButton text={s.output} /></div>
                  <pre tabIndex={0}>{s.output}</pre>
                </div>
              )}
              {!s.output && !s.error && s.status === "skipped" && <p className="faint text-xs">Saltato: nessuna condizione lo ha scelto.</p>}
            </li>
          );
        })}
      </ol>
    </Drawer>
  );
}
