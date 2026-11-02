import { useEffect, useState } from "react";
import { Link, navigate, useParams } from "../../app/router";
import { Button, Icon, MasterDetail, Page, PageHeader, Panel, Pill, Section, Segmented } from "../../shared/ui";
import type { AgentProfile, InferenceProfile, PolicyMode, SandboxMode } from "../../shared/api/types";
import { useModels } from "../models/api";
import { useRuntimes } from "../runtimes/api";
import { useTools } from "../capabilities/api";
import { useAgents, useProfiles, useSaveAgent, useSaveProfile } from "./api";

const MODES: { id: PolicyMode; label: string; hint: string }[] = [
  { id: "plan", label: "Piano", hint: "Solo lettura: propone, non modifica" },
  { id: "ask", label: "Chiedi", hint: "Conferma per ogni effetto collaterale" },
  { id: "auto-edit", label: "Modifica", hint: "Modifica i file da solo, chiede per i comandi" },
  { id: "auto", label: "Auto", hint: "Tutto, ma sempre dentro la sandbox" },
  { id: "yolo", label: "Senza freni", hint: "Nessuna conferma: sconsigliato" },
];

export function AgentsView() {
  const agents = useAgents();
  const profiles = useProfiles();
  const tools = useTools();
  const runtimes = useRuntimes();
  const models = useModels();
  const saveAgent = useSaveAgent();
  const saveProfile = useSaveProfile();
  const [draft, setDraft] = useState<AgentProfile | null>(null);

  const agentId = useParams().agentId;
  const list = agents.data ?? [];
  // Desktop shows the first agent while the URL names none; on phones the list is the page.
  const selected = list.find((a) => a.id === agentId) ?? (agentId ? undefined : list[0]);

  useEffect(() => {
    setDraft(selected ? structuredClone(selected) : null);
  }, [selected]);

  const create = () => {
    const id = `agent-${Date.now().toString(36)}`;
    const a: AgentProfile = { id, name: "Nuovo agente", description: "", inference_profile: profiles.data?.[0]?.id ?? "", system_prompt: "", tools: ["read_file", "list_dir", "todo"], mode: "ask", sandbox: "workspace-write", max_turns: 30 };
    saveAgent.mutate(a, { onSuccess: () => navigate(`/agents/${id}`) });
  };

  const toggleTool = (name: string) => {
    if (!draft) return;
    const all = draft.tools.includes("*");
    let next = all ? (tools.data ?? []).map((t) => t.name) : [...draft.tools];
    next = next.includes(name) ? next.filter((t) => t !== name) : [...next, name];
    setDraft({ ...draft, tools: next });
  };

  const detail = draft && (
    <Panel
      title={<>
        <Link to="/agents" className="md-back only-phone" aria-label="Torna agli agenti"><Icon name="back" size={16} /> Agenti</Link>
        {draft.name}
      </>}
      actions={<Button variant="primary" onClick={() => saveAgent.mutate(draft)} disabled={saveAgent.isPending}>Salva</Button>}
    >
      <div className="form-grid">
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
          <span className="field-label">Approvazioni</span>
          <Segmented className="seg-wrap" label="Modalità di approvazione" value={draft.mode}
            options={MODES.map((m) => ({ id: m.id, label: m.label, title: m.hint }))} onChange={(mode) => setDraft({ ...draft, mode })} />
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
          <span className="field-label">Tool</span>
          <div className="chips">
            {tools.data?.map((t) => {
              const on = draft.tools.includes("*") || draft.tools.includes(t.name);
              return (
                <button key={t.name} type="button" className={`chip chip-btn ${on ? "hot" : ""}`} aria-pressed={on} onClick={() => toggleTool(t.name)} title={t.description}>
                  {t.name}
                </button>
              );
            })}
          </div>
        </div>
      </div>
    </Panel>
  );

  return (
    <Page>
      <PageHeader
        eyebrow="Lavoro" title="Agenti"
        subtitle="Un agente unisce un profilo di inferenza, un set di tool e una politica di approvazione."
        actions={<Button variant="primary" icon="plus" onClick={create} disabled={saveAgent.isPending}>Nuovo agente</Button>}
      />

      <MasterDetail
        detailOpen={!!agentId}
        list={
          <div className="panel">
            <div className="list">
              {list.map((a) => (
                <Link key={a.id} to={`/agents/${encodeURIComponent(a.id)}`} className="list-item clickable" aria-current={a.id === selected?.id}>
                  <Icon name="agent" />
                  <div className="grow">
                    <b>{a.name}</b>
                    <div className="faint ellipsis text-xs">{a.description}</div>
                  </div>
                  <Pill>{a.mode}</Pill>
                </Link>
              ))}
            </div>
          </div>
        }
        detail={detail}
      />

      <Section title="Profili di inferenza" hint="Runtime, modello e parametri riutilizzabili da agenti e workflow.">
        <ProfilesTable profiles={profiles.data ?? []} runtimes={(runtimes.data ?? []).map((r) => r.id)} models={(models.data ?? []).map((m) => m.id)} onSave={(p) => saveProfile.mutate(p)} />
      </Section>
    </Page>
  );
}

function ProfilesTable({ profiles, runtimes, models, onSave }: { profiles: InferenceProfile[]; runtimes: string[]; models: string[]; onSave: (p: InferenceProfile) => void }) {
  const [edit, setEdit] = useState<InferenceProfile | null>(null);
  return (
    <div className="table-wrap">
      <table className="data" aria-label="Profili di inferenza">
        <thead><tr><th>Profilo</th><th>Runtime</th><th>Modello</th><th>Temperatura</th><th>Contesto</th><th /></tr></thead>
        <tbody>
          {profiles.map((p) => edit?.id === p.id ? (
            <tr key={p.id}>
              <td><input className="input" aria-label="Nome profilo" value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} /></td>
              <td><select className="input" aria-label="Runtime" value={edit.runtime_id} onChange={(e) => setEdit({ ...edit, runtime_id: e.target.value })}>{[edit.runtime_id, ...runtimes.filter((r) => r !== edit.runtime_id)].map((r) => <option key={r}>{r}</option>)}</select></td>
              <td><select className="input" aria-label="Modello" value={edit.model} onChange={(e) => setEdit({ ...edit, model: e.target.value })}>{[edit.model, ...models.filter((m) => m !== edit.model)].map((m) => <option key={m}>{m}</option>)}</select></td>
              <td><input className="input num" aria-label="Temperatura" type="number" step={0.05} min={0} max={2} value={edit.params.temperature ?? 0.7} onChange={(e) => setEdit({ ...edit, params: { ...edit.params, temperature: parseFloat(e.target.value) } })} /></td>
              <td><input className="input num" aria-label="Contesto" type="number" step={1024} min={512} value={edit.params.context ?? 8192} onChange={(e) => setEdit({ ...edit, params: { ...edit.params, context: parseInt(e.target.value, 10) } })} /></td>
              <td><div className="row"><Button variant="primary" size="sm" onClick={() => { onSave(edit); setEdit(null); }}>Salva</Button><Button size="sm" onClick={() => setEdit(null)}>Annulla</Button></div></td>
            </tr>
          ) : (
            <tr key={p.id}>
              <td><b>{p.name}</b><div className="faint text-xs">{p.description}</div></td>
              <td className="mono">{p.runtime_id}</td>
              <td className="mono">{p.model}</td>
              <td className="num">{p.params.temperature ?? "—"}</td>
              <td className="num">{p.params.context ? `${Math.round(p.params.context / 1024)}K` : "—"}</td>
              <td><Button size="sm" onClick={() => setEdit(structuredClone(p))}>Modifica</Button></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
