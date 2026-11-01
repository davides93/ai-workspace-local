import { useEffect, useState } from "react";
import { useApp } from "../state/app";
import { Icon, StatusPill, relTime, useAsync } from "../components/ui";
import type { WorkflowDef, WorkflowStep, WorkflowTrigger } from "../api/types";

export function triggerLabel(t: WorkflowTrigger): string {
  switch (t.kind) {
    case "manual": return "Manuale";
    case "interval": return t.seconds % 3600 === 0 ? `Ogni ${t.seconds / 3600} h` : `Ogni ${Math.round(t.seconds / 60)} min`;
    case "cron": return `Cron ${t.expr}`;
    case "file": return `Quando cambia ${t.path}`;
  }
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
  const { api, toast } = useApp();
  const wfs = useAsync(() => api.workflows(), [api]);
  const runs = useAsync(() => api.workflowRuns(), [api], 1500);
  const [sel, setSel] = useState<string | null>(null);
  const [source, setSource] = useState("");
  const [showSource, setShowSource] = useState(false);
  const wf = wfs.data?.find((w) => w.id === sel) ?? null;

  useEffect(() => {
    if (!sel && wfs.data?.length) setSel(wfs.data[0].id);
  }, [wfs.data, sel]);
  useEffect(() => {
    if (wf) setSource(JSON.stringify(wf, null, 2));
  }, [wf]);

  const runNow = async (w: WorkflowDef) => {
    try {
      await api.runWorkflow(w.id, w.inputs);
      toast(`Workflow "${w.name}" avviato`);
      runs.reload();
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  };
  const toggle = async (w: WorkflowDef) => {
    await api.saveWorkflow({ ...w, enabled: !w.enabled });
    wfs.reload();
  };
  const saveSource = async () => {
    try {
      const parsed = JSON.parse(source) as WorkflowDef;
      await api.saveWorkflow(parsed);
      toast("Workflow salvato");
      wfs.reload();
    } catch (e) {
      toast(`Definizione non valida: ${String((e as Error).message ?? e)}`, "crit");
    }
  };
  const create = async () => {
    const w: WorkflowDef = { id: `wf-${Date.now().toString(36)}`, name: "Nuovo workflow", enabled: false, trigger: { kind: "manual" }, inputs: {}, steps: [{ id: "step1", kind: "agent", agent: "operator", prompt: "Descrivi qui il compito" }] };
    await api.saveWorkflow(w);
    await wfs.reload();
    setSel(w.id);
    setShowSource(true);
  };

  const lastRun = runs.data?.find((r) => r.workflow_id === sel);

  return (
    <div className="page">
      <div className="page-head">
        <div className="titles">
          <span className="eyebrow">Lavoro</span>
          <h1>Workflow</h1>
          <span className="muted">Task operativi ripetibili: trigger, passi con tool o agenti, conferme umane.</span>
        </div>
        <div className="actions"><button className="btn primary" onClick={create}><Icon name="plus" size={15} /> Nuovo workflow</button></div>
      </div>

      <div className="cols-2" style={{ gridTemplateColumns: "minmax(0, 0.9fr) minmax(0, 1.5fr)" }}>
        <div className="panel" style={{ alignSelf: "start" }}>
          <div className="list">
            {wfs.data?.map((w) => (
              <div key={w.id} className="list-item clickable" aria-current={w.id === sel} onClick={() => setSel(w.id)} role="button" tabIndex={0} onKeyDown={(e) => e.key === "Enter" && setSel(w.id)}>
                <Icon name="flow" />
                <div className="grow">
                  <b>{w.name}</b>
                  <div className="faint" style={{ fontSize: "var(--fs-xs)" }}>{triggerLabel(w.trigger)} · {w.steps.length} passi</div>
                </div>
                <span className="toggle" onClick={(e) => e.stopPropagation()}>
                  <input type="checkbox" aria-label={`Attiva ${w.name}`} checked={w.enabled} onChange={() => toggle(w)} />
                </span>
              </div>
            ))}
          </div>
        </div>

        {wf && (
          <div className="stack" style={{ gap: 16 }}>
            <div className="panel">
              <div className="panel-head">
                <div className="grow">
                  <h2>{wf.name}</h2>
                  <span className="muted" style={{ fontSize: "var(--fs-sm)" }}>{wf.description}</span>
                </div>
                <button className="btn" onClick={() => setShowSource((v) => !v)}>{showSource ? "Vista passi" : "Sorgente"}</button>
                <button className="btn primary" onClick={() => runNow(wf)}><Icon name="play" size={14} /> Esegui ora</button>
              </div>
              <div className="panel-body">
                <div className="row" style={{ marginBottom: 14 }}>
                  <span className="pill info"><Icon name="refresh" size={13} /> {triggerLabel(wf.trigger)}</span>
                  {Object.entries(wf.inputs).map(([k, v]) => <span key={k} className="chip">{k}={String(v)}</span>)}
                </div>
                {showSource ? (
                  <div className="stack">
                    <textarea className="input" aria-label="Definizione del workflow (JSON)" rows={18} value={source} onChange={(e) => setSource(e.target.value)} />
                    <div className="row"><button className="btn primary" onClick={saveSource}>Salva definizione</button><span className="faint" style={{ fontSize: "var(--fs-xs)" }}>Formato JSON. Il backend accetta anche file YAML in .aiw/workflows/.</span></div>
                  </div>
                ) : (
                  <div className="wf-steps">
                    {wf.steps.map((s, i) => {
                      const st = lastRun?.steps.find((x) => x.id === s.id);
                      return (
                        <div key={s.id} className={`wf-step ${st?.status ?? ""}`}>
                          <div className="rail-dot"><b>{i + 1}</b>{i < wf.steps.length - 1 && <i />}</div>
                          <div className="body">
                            <div className="box">
                              <div className="row">
                                <Icon name={STEP_META[s.kind].icon} size={15} />
                                <b>{STEP_META[s.kind].label}</b>
                                <span className="mono faint" style={{ fontSize: "var(--fs-xs)" }}>{s.id}</span>
                                {st && <span style={{ marginLeft: "auto" }}><StatusPill status={st.status} /></span>}
                              </div>
                              <div className="mono muted" style={{ fontSize: "var(--fs-xs)", marginTop: 6, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{stepSummary(s)}</div>
                            </div>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>

            <div className="panel">
              <div className="panel-head"><h3 className="grow">Esecuzioni recenti</h3></div>
              <div className="list">
                {runs.data?.filter((r) => r.workflow_id === wf.id).slice(0, 6).map((r) => (
                  <div className="list-item" key={r.id}>
                    <StatusPill status={r.status} />
                    <span className="mono faint" style={{ fontSize: "var(--fs-xs)" }}>{r.id}</span>
                    <span className="grow faint" style={{ fontSize: "var(--fs-xs)" }}>{relTime(r.started_at)}</span>
                    <div className="chips">{r.steps.map((s) => <span key={s.id} className={`chip ${s.status === "done" ? "" : s.status === "running" ? "hot" : ""}`}>{s.id}:{s.status}</span>)}</div>
                  </div>
                ))}
                {!runs.data?.some((r) => r.workflow_id === wf.id) && <div className="list-item faint">Nessuna esecuzione.</div>}
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
