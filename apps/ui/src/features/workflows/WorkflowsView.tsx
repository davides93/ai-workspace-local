import { useEffect, useState } from "react";
import { Link, navigate, useParams } from "../../app/router";
import { useApi } from "../../shared/api/context";
import { relTime } from "../../shared/lib/format";
import { serviceBaseUrl } from "../../shared/lib/baseUrl";
import { toast } from "../../shared/lib/toast";
import { errorMessage } from "../../shared/lib/errors";
import { Button, Chip, CopyField, Icon, MasterDetail, Page, PageHeader, Panel, Pill, StatusPill, Toggle } from "../../shared/ui";
import type { WorkflowDef, WorkflowStep, WorkflowTrigger } from "../../shared/api/types";
import { useRunWorkflow, useSaveWorkflow, useWorkflowRuns, useWorkflows } from "./api";

export function triggerLabel(t: WorkflowTrigger): string {
  switch (t.kind) {
    case "manual": return "Manuale";
    case "interval": return t.seconds % 3600 === 0 ? `Ogni ${t.seconds / 3600} h` : `Ogni ${Math.round(t.seconds / 60)} min`;
    case "cron": return `Cron ${t.expr}`;
    case "file": return `Quando cambia ${t.path}`;
    case "webhook": return "Webhook";
  }
}

function WebhookInfo({ id, secret }: { id: string; secret?: string }) {
  const api = useApi();
  const url = `${serviceBaseUrl({ native: api.capabilities.native })}/hooks/${encodeURIComponent(id)}`;
  return (
    <div className="webhook-info">
      <CopyField label="URL (POST)" value={url} />
      {secret ? <CopyField label="Segreto" value={secret} secret /> : <span className="faint text-sm">Segreto generato al salvataggio.</span>}
      <span className="faint text-xs">
        Invia il segreto nell'header <span className="mono">X-AIW-Secret</span> oppure firma il corpo con <span className="mono">X-Hub-Signature-256</span> (HMAC-SHA256, come GitHub). Il JSON ricevuto diventa gli input del workflow.
      </span>
    </div>
  );
}

const STEP_META: Record<WorkflowStep["kind"], { label: string; icon: string }> = {
  tool: { label: "Tool", icon: "tool" },
  agent: { label: "Agente", icon: "agent" },
  llm: { label: "LLM", icon: "sparkle" },
  approval: { label: "Conferma", icon: "shield" },
  condition: { label: "Condizione", icon: "flow" },
};

function stepSummary(s: WorkflowStep): string {
  switch (s.kind) {
    case "tool": return `${s.tool} ${JSON.stringify(s.args)}`;
    case "agent": return `${s.agent}: ${s.prompt}`;
    case "llm": return `${s.profile}: ${s.prompt}`;
    case "approval": return s.message;
    case "condition": return `se ${s.if} → ${s.then.join(", ")}${s.else ? ` altrimenti ${s.else.join(", ")}` : ""}`;
  }
}

export function WorkflowsView() {
  const wfs = useWorkflows();
  const runs = useWorkflowRuns();
  const save = useSaveWorkflow();
  const toggleSave = useSaveWorkflow({ silent: true });
  const runNow = useRunWorkflow();
  const [source, setSource] = useState("");
  const [showSource, setShowSource] = useState(false);

  const workflowId = useParams().workflowId;
  const list = wfs.data ?? [];
  const wf = list.find((w) => w.id === workflowId) ?? (workflowId ? null : list[0] ?? null);

  useEffect(() => {
    if (wf) setSource(JSON.stringify(wf, null, 2));
  }, [wf]);

  const saveSource = () => {
    try {
      save.mutate(JSON.parse(source) as WorkflowDef);
    } catch (e) {
      toast(`Definizione non valida: ${errorMessage(e)}`, "crit");
    }
  };
  const create = () => {
    const w: WorkflowDef = { id: `wf-${Date.now().toString(36)}`, name: "Nuovo workflow", enabled: false, trigger: { kind: "manual" }, inputs: {}, steps: [{ id: "step1", kind: "agent", agent: "operator", prompt: "Descrivi qui il compito" }] };
    save.mutate(w, { onSuccess: () => { navigate(`/workflows/${w.id}`); setShowSource(true); } });
  };

  const lastRun = runs.data?.find((r) => r.workflow_id === wf?.id);
  const wfRuns = runs.data?.filter((r) => r.workflow_id === wf?.id) ?? [];

  return (
    <Page>
      <PageHeader
        eyebrow="Lavoro" title="Workflow"
        subtitle="Task operativi ripetibili: trigger, passi con tool o agenti, conferme umane."
        actions={<Button variant="primary" icon="plus" onClick={create} disabled={save.isPending}>Nuovo workflow</Button>}
      />

      <MasterDetail
        ratio="wide"
        detailOpen={!!workflowId}
        list={
          <div className="panel">
            <div className="list">
              {list.map((w) => (
                <div key={w.id} className="list-item" aria-current={w.id === wf?.id}>
                  <Link to={`/workflows/${encodeURIComponent(w.id)}`} className="list-link">
                    <Icon name="flow" />
                    <div className="grow">
                      <b>{w.name}</b>
                      <div className="faint text-xs">{triggerLabel(w.trigger)} · {w.steps.length} passi</div>
                    </div>
                  </Link>
                  <Toggle label={`Attiva ${w.name}`} checked={w.enabled} onChange={(enabled) => toggleSave.mutate({ ...w, enabled })} />
                </div>
              ))}
            </div>
          </div>
        }
        detail={wf && (
          <div className="stack stack-lg">
            <Panel
              title={<>
                <Link to="/workflows" className="md-back only-phone" aria-label="Torna ai workflow"><Icon name="back" size={16} /> Workflow</Link>
                {wf.name}
              </>}
              actions={<>
                <Button onClick={() => setShowSource((v) => !v)}>{showSource ? "Vista passi" : "Sorgente"}</Button>
                <Button variant="primary" icon="play" onClick={() => runNow.mutate(wf)} disabled={runNow.isPending}>Esegui ora</Button>
              </>}
            >
              {wf.description && <p className="muted text-sm mb-3">{wf.description}</p>}
              <div className="row mb-3">
                <Pill tone="info"><Icon name="refresh" size={13} /> {triggerLabel(wf.trigger)}</Pill>
                {Object.entries(wf.inputs).map(([k, v]) => <Chip key={k}>{k}={String(v)}</Chip>)}
              </div>
              {wf.trigger.kind === "webhook" && <WebhookInfo id={wf.id} secret={wf.trigger.secret} />}
              {showSource ? (
                <div className="stack">
                  <textarea className="input" aria-label="Definizione del workflow (JSON)" rows={18} value={source} onChange={(e) => setSource(e.target.value)} />
                  <div className="row"><Button variant="primary" onClick={saveSource}>Salva definizione</Button><span className="faint text-xs">Formato JSON. Il backend accetta anche file YAML in .aiw/workflows/.</span></div>
                </div>
              ) : (
                <ol className="wf-steps">
                  {wf.steps.map((s, i) => {
                    const st = lastRun?.steps.find((x) => x.id === s.id);
                    return (
                      <li key={s.id} className={`wf-step ${st?.status ?? ""}`}>
                        <div className="rail-dot"><b>{i + 1}</b>{i < wf.steps.length - 1 && <i />}</div>
                        <div className="body">
                          <div className="box">
                            <div className="row">
                              <Icon name={STEP_META[s.kind].icon} size={15} />
                              <b>{STEP_META[s.kind].label}</b>
                              <span className="mono faint text-xs">{s.id}</span>
                              {st && <span className="ml-auto"><StatusPill status={st.status} /></span>}
                            </div>
                            <div className="mono muted text-xs step-summary">{stepSummary(s)}</div>
                          </div>
                        </div>
                      </li>
                    );
                  })}
                </ol>
              )}
            </Panel>

            <Panel title="Esecuzioni recenti" padded={false}>
              <div className="list">
                {wfRuns.slice(0, 6).map((r) => (
                  <div className="list-item" key={r.id}>
                    <StatusPill status={r.status} />
                    <span className="mono faint text-xs">{r.id}</span>
                    <span className="grow faint text-xs">{relTime(r.started_at)}</span>
                    <div className="chips">{r.steps.map((s) => <Chip key={s.id} hot={s.status === "running"}>{s.id}:{s.status}</Chip>)}</div>
                  </div>
                ))}
                {wfRuns.length === 0 && <div className="list-item faint">Nessuna esecuzione.</div>}
              </div>
            </Panel>
          </div>
        )}
      />
    </Page>
  );
}
