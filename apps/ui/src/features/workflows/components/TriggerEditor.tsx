import { useApi } from "../../../shared/api/context";
import { CodeBlock, CopyField, Field, Icon, Input, Select } from "../../../shared/ui";
import { formatDateTime } from "../../../shared/lib/format";
import { serviceBaseUrl } from "../../../shared/lib/baseUrl";
import type { WorkflowRun } from "../../../shared/api/types";
import { describeCron, nextCronRun, parseCron } from "../model/cron";
import type { Issue, TriggerDraft } from "../model/draft";
import { joinInterval, MIN_INTERVAL_S, TRIGGER_KINDS, UNIT_LABEL, type IntervalUnit } from "../model/triggers";

const UNITS: IntervalUnit[] = ["s", "min", "h"];
const issue = (issues: Issue[], field: string) => issues.find((i) => i.scope === "trigger" && i.field === field);

function Webhook({ id, secret }: { id: string; secret: string }) {
  const api = useApi();
  const url = id ? `${serviceBaseUrl({ native: api.capabilities.native })}/hooks/${encodeURIComponent(id)}` : "";
  return (
    <div className="webhook-info">
      {url ? <CopyField label="Indirizzo (POST)" value={url} /> : <p className="faint text-sm">L'indirizzo compare dopo il primo salvataggio.</p>}
      {secret
        ? <CopyField label="Segreto" value={secret} secret hint="Mostralo solo a chi deve chiamare il workflow." />
        : <p className="faint text-sm">Il segreto viene generato dal backend al salvataggio.</p>}
      <p className="faint text-xs">
        Invia il segreto nell'header <span className="mono">X-AIW-Secret</span> oppure firma il corpo con <span className="mono">X-Hub-Signature-256</span> (HMAC-SHA256, come GitHub).
        Il JSON ricevuto diventa gli input del workflow; il workflow deve essere attivo.
      </p>
      {url && <CodeBlock title="Esempio" code={`curl -X POST ${url} \\\n  -H "X-AIW-Secret: <segreto>" \\\n  -H "Content-Type: application/json" \\\n  -d '{"chiave": "valore"}'`} />}
    </div>
  );
}

/** Trigger kind and its fields, with the next scheduled start where it can be computed. */
export function TriggerEditor({ trigger, onChange, issues, workflowId, savedSecret, enabled, lastRun }: {
  trigger: TriggerDraft;
  onChange: (t: TriggerDraft) => void;
  issues: Issue[];
  /** Empty while the workflow has not been saved. */
  workflowId: string;
  savedSecret: string;
  enabled: boolean;
  lastRun?: WorkflowRun;
}) {
  const kind = TRIGGER_KINDS.find((k) => k.kind === trigger.kind)!;
  const setKind = (k: TriggerDraft["kind"]) => {
    if (k === trigger.kind) return;
    onChange(
      k === "manual" ? { kind: "manual" }
      : k === "cron" ? { kind: "cron", expr: "0 8 * * 1-5" }
      : k === "interval" ? { kind: "interval", amount: 1, unit: "h" }
      : k === "file" ? { kind: "file", path: "" }
      : { kind: "webhook" },
    );
  };
  const cron = trigger.kind === "cron" ? parseCron(trigger.expr) : null;
  const next = trigger.kind === "cron" && cron?.ok ? nextCronRun(trigger.expr, new Date()) : null;
  const seconds = trigger.kind === "interval" ? joinInterval(trigger.amount, trigger.unit) : 0;
  const last = lastRun ? new Date(lastRun.started_at) : null;

  return (
    <div className="stack stack-lg">
      <Field label="Quando parte" htmlFor="wf-trigger" hint={kind.hint}>
        <Select id="wf-trigger" value={trigger.kind} onChange={(e) => setKind(e.target.value as TriggerDraft["kind"])}>
          {TRIGGER_KINDS.map((k) => <option key={k.kind} value={k.kind}>{k.label}</option>)}
        </Select>
      </Field>

      {trigger.kind === "cron" && (
        <div className="stack">
          <Field label="Espressione cron" htmlFor="wf-cron" error={issue(issues, "expr")?.message} hint="minuto ora giorno-del-mese mese giorno-della-settimana (0 o 7 = domenica)">
            <Input id="wf-cron" mono value={trigger.expr} onChange={(e) => onChange({ kind: "cron", expr: e.target.value })} aria-invalid={!!issue(issues, "expr")} spellCheck={false} placeholder="0 8 * * 1-5" />
          </Field>
          {cron?.ok && (
            <p className="trigger-next text-sm" role="status">
              <Icon name="refresh" size={14} />
              <span>
                {describeCron(trigger.expr) ?? "Pianificazione personalizzata"}
                {enabled && next ? <> — prossima esecuzione <b>{formatDateTime(next)}</b></> : null}
                {!enabled && <span className="muted"> — il workflow è disattivato, quindi non parte da solo</span>}
                {enabled && !next && <span className="muted"> — nessuna esecuzione prevista</span>}
              </span>
            </p>
          )}
          <p className="faint text-xs">L'orario è quello del computer su cui gira il servizio. Giorno del mese e giorno della settimana, se indicati entrambi, devono combaciare insieme.</p>
        </div>
      )}

      {trigger.kind === "interval" && (
        <div className="stack">
          <div className="interval-row">
            <Field label="Ogni" htmlFor="wf-interval" error={issue(issues, "amount")?.level === "error" ? issue(issues, "amount")?.message : undefined}>
              <Input id="wf-interval" className="num" type="number" min={1} step={1} value={Number.isNaN(trigger.amount) ? "" : trigger.amount}
                onChange={(e) => onChange({ ...trigger, amount: e.target.value === "" ? Number.NaN : Number(e.target.value) })} />
            </Field>
            <Field label="Unità" htmlFor="wf-interval-unit">
              <Select id="wf-interval-unit" value={trigger.unit} onChange={(e) => onChange({ ...trigger, unit: e.target.value as IntervalUnit })}>
                {UNITS.map((u) => <option key={u} value={u}>{UNIT_LABEL[u]}</option>)}
              </Select>
            </Field>
          </div>
          {issue(issues, "amount")?.level === "warn" && <p className="row-warn">{issue(issues, "amount")?.message}</p>}
          <p className="faint text-xs">
            Il conteggio parte all'avvio del servizio (non prima di {MIN_INTERVAL_S} secondi).
            {last && seconds > 0 && enabled && <> Ultima esecuzione: {formatDateTime(last)}.</>}
          </p>
        </div>
      )}

      {trigger.kind === "file" && (
        <Field label="File o cartella" htmlFor="wf-file" error={issue(issues, "path")?.message} hint="Parte quando la data di modifica cambia. Si può usare ~ per la cartella personale.">
          <Input id="wf-file" mono value={trigger.path} onChange={(e) => onChange({ kind: "file", path: e.target.value })} placeholder="~/Download/ordini.csv" aria-invalid={!!issue(issues, "path")} spellCheck={false} />
        </Field>
      )}

      {trigger.kind === "webhook" && <Webhook id={workflowId} secret={savedSecret} />}
    </div>
  );
}
