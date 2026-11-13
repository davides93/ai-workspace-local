import { useApi } from "../../../shared/api/context";
import { CodeBlock, CopyField, Field, Icon, Input, Select } from "../../../shared/ui";
import { formatDateTime } from "../../../shared/lib/format";
import { serviceBaseUrl } from "../../../shared/lib/baseUrl";
import type { WorkflowRun } from "../../../shared/api/types";
import { describeCron, nextCronRun, parseCron } from "../model/cron";
import type { Issue, TriggerDraft } from "../model/draft";
import { joinInterval, MIN_INTERVAL_S, TRIGGER_KINDS, UNIT_LABEL, type IntervalUnit } from "../model/triggers";

import { t as tr, tNode as trNode } from "../../../shared/i18n";
const UNITS: IntervalUnit[] = ["s", "min", "h"];
const issue = (issues: Issue[], field: string) => issues.find((i) => i.scope === "trigger" && i.field === field);

function Webhook({ id, secret }: { id: string; secret: string }) {
  const api = useApi();
  const url = id ? `${serviceBaseUrl({ native: api.capabilities.native })}/hooks/${encodeURIComponent(id)}` : "";
  return (
    <div className="webhook-info">
      {url ? <CopyField label={tr("workflows.indirizzo_post")} value={url} /> : <p className="faint text-sm">{tr("workflows.l_indirizzo_compare_dopo_il")}</p>}
      {secret
        ? <CopyField label={tr("workflows.segreto")} value={secret} secret hint={tr("workflows.mostralo_solo_a_chi_deve")} />
        : <p className="faint text-sm">{tr("workflows.il_segreto_viene_generato_dal")}</p>}
      <p className="faint text-xs">
        {trNode("workflows.invia_il_segreto_nell_header", { span: <span className="mono">X-AIW-Secret</span>, span2: <span className="mono">X-Hub-Signature-256</span> })}</p>
      {url && <CodeBlock title={tr("workflows.esempio")} code={`curl -X POST ${url} \\\n  -H "X-AIW-Secret: ${tr("workflows.example_secret")}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"${tr("workflows.example_key")}": "${tr("workflows.example_value")}"}'`} />}
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
      <Field label={tr("workflows.quando_parte")} htmlFor="wf-trigger" hint={kind.hint}>
        <Select id="wf-trigger" value={trigger.kind} onChange={(e) => setKind(e.target.value as TriggerDraft["kind"])}>
          {TRIGGER_KINDS.map((k) => <option key={k.kind} value={k.kind}>{k.label}</option>)}
        </Select>
      </Field>

      {trigger.kind === "cron" && (
        <div className="stack">
          <Field label={tr("workflows.espressione_cron")} htmlFor="wf-cron" error={issue(issues, "expr")?.message} hint={tr("workflows.minuto_ora_giorno_del_mese")}>
            <Input id="wf-cron" mono value={trigger.expr} onChange={(e) => onChange({ kind: "cron", expr: e.target.value })} aria-invalid={!!issue(issues, "expr")} spellCheck={false} placeholder="0 8 * * 1-5" />
          </Field>
          {cron?.ok && (
            <p className="trigger-next text-sm" role="status">
              <Icon name="refresh" size={14} />
              <span>
                {describeCron(trigger.expr) ?? tr("workflows.pianificazione_personalizzata")}
                {enabled && next ? <>{trNode("workflows.next_run_prefix", { when: <b>{formatDateTime(next)}</b> })}</> : null}
                {!enabled && <span className="muted">{" "}{tr("workflows.il_workflow_e_disattivato_quindi")}</span>}
                {enabled && !next && <span className="muted">{" "}{tr("workflows.nessuna_esecuzione_prevista")}</span>}
              </span>
            </p>
          )}
          <p className="faint text-xs">{tr("workflows.l_orario_e_quello_del")}</p>
        </div>
      )}

      {trigger.kind === "interval" && (
        <div className="stack">
          <div className="interval-row">
            <Field label={tr("workflows.ogni")} htmlFor="wf-interval" error={issue(issues, "amount")?.level === "error" ? issue(issues, "amount")?.message : undefined}>
              <Input id="wf-interval" className="num" type="number" min={1} step={1} value={Number.isNaN(trigger.amount) ? "" : trigger.amount}
                onChange={(e) => onChange({ ...trigger, amount: e.target.value === "" ? Number.NaN : Number(e.target.value) })} />
            </Field>
            <Field label={tr("workflows.unita")} htmlFor="wf-interval-unit">
              <Select id="wf-interval-unit" value={trigger.unit} onChange={(e) => onChange({ ...trigger, unit: e.target.value as IntervalUnit })}>
                {UNITS.map((u) => <option key={u} value={u}>{UNIT_LABEL[u]}</option>)}
              </Select>
            </Field>
          </div>
          {issue(issues, "amount")?.level === "warn" && <p className="row-warn">{issue(issues, "amount")?.message}</p>}
          <p className="faint text-xs">
            {tr("workflows.interval_note", { seconds: MIN_INTERVAL_S })}
            {last && seconds > 0 && enabled && <>{" "}{tr("workflows.ultima_esecuzione", { time: formatDateTime(last) })}</>}
          </p>
        </div>
      )}

      {trigger.kind === "file" && (
        <Field label={tr("workflows.file_o_cartella")} htmlFor="wf-file" error={issue(issues, "path")?.message} hint={tr("workflows.parte_quando_la_data_di")}>
          <Input id="wf-file" mono value={trigger.path} onChange={(e) => onChange({ kind: "file", path: e.target.value })} placeholder="~/Download/ordini.csv" aria-invalid={!!issue(issues, "path")} spellCheck={false} />
        </Field>
      )}

      {trigger.kind === "webhook" && <Webhook id={workflowId} secret={savedSecret} />}
    </div>
  );
}
