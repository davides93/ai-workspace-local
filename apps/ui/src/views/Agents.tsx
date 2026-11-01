import { useEffect, useState } from "react";
import { useApp } from "../state/app";
import { Icon, useAsync } from "../components/ui";
import type { AgentProfile, InferenceProfile, PolicyMode, SandboxMode } from "../api/types";

const MODES: { id: PolicyMode; label: string; hint: string }[] = [
  { id: "plan", label: "Piano", hint: "Solo lettura: propone, non modifica" },
  { id: "ask", label: "Chiedi", hint: "Conferma per ogni effetto collaterale" },
  { id: "auto-edit", label: "Modifica", hint: "Modifica i file da solo, chiede per i comandi" },
  { id: "auto", label: "Auto", hint: "Tutto, ma sempre dentro la sandbox" },
  { id: "yolo", label: "Senza freni", hint: "Nessuna conferma: sconsigliato" },
];

export function AgentsView() {
  const { api, toast } = useApp();
  const agents = useAsync(() => api.agents(), [api]);
  const profiles = useAsync(() => api.profiles(), [api]);
  const tools = useAsync(() => api.tools(), [api]);
  const runtimes = useAsync(() => api.runtimes(), [api]);
  const models = useAsync(() => api.models(), [api]);
  const [sel, setSel] = useState<string | null>(null);
  const [draft, setDraft] = useState<AgentProfile | null>(null);

  useEffect(() => {
    if (!sel && agents.data?.length) setSel(agents.data[0].id);
  }, [agents.data, sel]);
  useEffect(() => {
    const a = agents.data?.find((x) => x.id === sel);
    setDraft(a ? structuredClone(a) : null);
  }, [sel, agents.data]);

  const save = async () => {
    if (!draft) return;
    try {
      await api.saveAgent(draft);
      toast(`Agente "${draft.name}" salvato`);
      agents.reload();
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  };

  const create = async () => {
    const id = `agent-${Date.now().toString(36)}`;
    const a: AgentProfile = { id, name: "Nuovo agente", description: "", inference_profile: profiles.data?.[0]?.id ?? "", system_prompt: "", tools: ["read_file", "list_dir", "todo"], mode: "ask", sandbox: "workspace-write", max_turns: 30 };
    await api.saveAgent(a);
    await agents.reload();
    setSel(id);
  };

  const toggleTool = (name: string) => {
    if (!draft) return;
    const all = draft.tools.includes("*");
    let next = all ? (tools.data ?? []).map((t) => t.name) : [...draft.tools];
    next = next.includes(name) ? next.filter((t) => t !== name) : [...next, name];
    setDraft({ ...draft, tools: next });
  };

  return (
    <div className="page">
      <div className="page-head">
        <div className="titles">
          <span className="eyebrow">Lavoro</span>
          <h1>Agenti</h1>
          <span className="muted">Un agente unisce un profilo di inferenza, un set di tool e una politica di approvazione.</span>
        </div>
        <div className="actions"><button className="btn primary" onClick={create}><Icon name="plus" size={15} /> Nuovo agente</button></div>
      </div>

      <div className="cols-2" style={{ gridTemplateColumns: "minmax(0, 0.8fr) minmax(0, 1.6fr)" }}>
        <div className="panel" style={{ alignSelf: "start" }}>
          <div className="list">
            {agents.data?.map((a) => (
              <div key={a.id} className="list-item clickable" aria-current={a.id === sel} onClick={() => setSel(a.id)} role="button" tabIndex={0}
                onKeyDown={(e) => e.key === "Enter" && setSel(a.id)}>
                <Icon name="agent" />
                <div className="grow">
                  <b>{a.name}</b>
                  <div className="faint ellipsis" style={{ fontSize: "var(--fs-xs)" }}>{a.description}</div>
                </div>
                <span className="pill neutral">{a.mode}</span>
              </div>
            ))}
          </div>
        </div>

        {draft && (
          <div className="panel">
            <div className="panel-head">
              <h2 className="grow">{draft.name}</h2>
              <button className="btn primary" onClick={save}>Salva</button>
            </div>
            <div className="panel-body form-grid">
              <div className="cols-2">
                <div className="field">
                  <label htmlFor="ag-name">Nome</label>
                  <input id="ag-name" className="input" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
                </div>
                <div className="field">
                  <label htmlFor="ag-profile">Profilo di inferenza</label>
                  <select id="ag-profile" className="input" value={draft.inference_profile} onChange={(e) => setDraft({ ...draft, inference_profile: e.target.value })}>
                    {profiles.data?.map((p) => <option key={p.id} value={p.id}>{p.name} — {p.runtime_id} · {p.model}</option>)}
                  </select>
                </div>
              </div>
              <div className="field">
                <label htmlFor="ag-desc">Descrizione</label>
                <input id="ag-desc" className="input" value={draft.description} onChange={(e) => setDraft({ ...draft, description: e.target.value })} />
              </div>
              <div className="field">
                <label htmlFor="ag-prompt">Istruzioni di sistema</label>
                <textarea id="ag-prompt" className="input" rows={4} value={draft.system_prompt} onChange={(e) => setDraft({ ...draft, system_prompt: e.target.value })} />
                <span className="hint">Si aggiungono ad AGENTS.md del workspace e all'elenco delle skill disponibili.</span>
              </div>
              <div className="field">
                <label>Approvazioni</label>
                <div className="seg" role="group" aria-label="Modalità di approvazione" style={{ flexWrap: "wrap" }}>
                  {MODES.map((m) => <button key={m.id} aria-pressed={draft.mode === m.id} title={m.hint} onClick={() => setDraft({ ...draft, mode: m.id })}>{m.label}</button>)}
                </div>
                <span className="hint">{MODES.find((m) => m.id === draft.mode)?.hint}</span>
              </div>
              <div className="cols-2">
                <div className="field">
                  <label htmlFor="ag-sandbox">Sandbox</label>
                  <select id="ag-sandbox" className="input" value={draft.sandbox} onChange={(e) => setDraft({ ...draft, sandbox: e.target.value as SandboxMode })}>
                    <option value="read-only">read-only</option>
                    <option value="workspace-write">workspace-write</option>
                    <option value="off">off (nessun contenimento)</option>
                  </select>
                </div>
                <div className="field">
                  <label htmlFor="ag-turns">Turni massimi</label>
                  <input id="ag-turns" className="input num" type="number" min={1} max={500} value={draft.max_turns} onChange={(e) => setDraft({ ...draft, max_turns: parseInt(e.target.value || "1", 10) })} />
                </div>
              </div>
              <div className="field">
                <label>Tool</label>
                <div className="chips">
                  {tools.data?.map((t) => {
                    const on = draft.tools.includes("*") || draft.tools.includes(t.name);
                    return (
                      <button key={t.name} type="button" className={`chip ${on ? "hot" : ""}`} aria-pressed={on} onClick={() => toggleTool(t.name)} style={{ cursor: "pointer" }} title={t.description}>
                        {t.name}
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      <section className="section" style={{ marginTop: 28 }}>
        <div className="section-head">
          <h2>Profili di inferenza</h2>
          <span className="muted">Runtime, modello e parametri riutilizzabili da agenti e workflow.</span>
        </div>
        <ProfilesTable profiles={profiles.data ?? []} runtimes={(runtimes.data ?? []).map((r) => r.id)} models={(models.data ?? []).map((m) => m.id)} onSave={async (p) => { await api.saveProfile(p); profiles.reload(); toast("Profilo salvato"); }} />
      </section>
    </div>
  );
}

function ProfilesTable({ profiles, runtimes, models, onSave }: { profiles: InferenceProfile[]; runtimes: string[]; models: string[]; onSave: (p: InferenceProfile) => void }) {
  const [edit, setEdit] = useState<InferenceProfile | null>(null);
  return (
    <div className="table-wrap">
      <table className="data">
        <thead><tr><th>Profilo</th><th>Runtime</th><th>Modello</th><th>Temperatura</th><th>Contesto</th><th /></tr></thead>
        <tbody>
          {profiles.map((p) => edit?.id === p.id ? (
            <tr key={p.id}>
              <td><input className="input" aria-label="Nome profilo" value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} /></td>
              <td><select className="input" aria-label="Runtime" value={edit.runtime_id} onChange={(e) => setEdit({ ...edit, runtime_id: e.target.value })}>{[edit.runtime_id, ...runtimes.filter((r) => r !== edit.runtime_id)].map((r) => <option key={r}>{r}</option>)}</select></td>
              <td><select className="input" aria-label="Modello" value={edit.model} onChange={(e) => setEdit({ ...edit, model: e.target.value })}>{[edit.model, ...models.filter((m) => m !== edit.model)].map((m) => <option key={m}>{m}</option>)}</select></td>
              <td><input className="input num" aria-label="Temperatura" type="number" step={0.05} min={0} max={2} value={edit.params.temperature ?? 0.7} onChange={(e) => setEdit({ ...edit, params: { ...edit.params, temperature: parseFloat(e.target.value) } })} /></td>
              <td><input className="input num" aria-label="Contesto" type="number" step={1024} min={512} value={edit.params.context ?? 8192} onChange={(e) => setEdit({ ...edit, params: { ...edit.params, context: parseInt(e.target.value, 10) } })} /></td>
              <td><div className="row"><button className="btn primary sm" onClick={() => { onSave(edit); setEdit(null); }}>Salva</button><button className="btn sm" onClick={() => setEdit(null)}>Annulla</button></div></td>
            </tr>
          ) : (
            <tr key={p.id}>
              <td><b>{p.name}</b><div className="faint" style={{ fontSize: "var(--fs-xs)" }}>{p.description}</div></td>
              <td className="mono">{p.runtime_id}</td>
              <td className="mono">{p.model}</td>
              <td className="num">{p.params.temperature ?? "—"}</td>
              <td className="num">{p.params.context ? `${Math.round(p.params.context / 1024)}K` : "—"}</td>
              <td><button className="btn sm" onClick={() => setEdit(structuredClone(p))}>Modifica</button></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
