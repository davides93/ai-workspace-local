import { Link } from "../../../app/router";
import { Field, Icon, KeyValue, Pill, Select, StatusPill } from "../../../shared/ui";
import { formatContext } from "../../../shared/lib/format";
import type { AgentProfile, InferenceProfile, RuntimeInfo } from "../../../shared/api/types";
import type { AgentIssue } from "../model/agents";

/** Which inference profile the agent runs on, and what that profile means in practice. */
export function ModelSection({ agent, profiles, runtimes, issues, onChange }: {
  agent: AgentProfile;
  profiles: InferenceProfile[];
  runtimes: RuntimeInfo[];
  issues: AgentIssue[];
  onChange: (patch: Partial<AgentProfile>) => void;
}) {
  const profile = profiles.find((p) => p.id === agent.inference_profile);
  const runtime = runtimes.find((r) => r.id === profile?.runtime_id);
  const error = issues.find((i) => i.field === "inference_profile" && i.level === "error")?.message;
  const missing = !!agent.inference_profile && !profile && profiles.length > 0;
  return (
    <div className="form-grid">
      <Field label="Profilo di inferenza" htmlFor="ag-profile" error={error} hint="Decide runtime, modello e parametri di generazione.">
        <Select id="ag-profile" value={agent.inference_profile} onChange={(e) => onChange({ inference_profile: e.target.value })} aria-invalid={!!error}>
          {missing && <option value={agent.inference_profile}>{agent.inference_profile} (non esiste più)</option>}
          {!agent.inference_profile && <option value="">Scegli un profilo…</option>}
          {profiles.map((p) => <option key={p.id} value={p.id}>{p.name} — {p.runtime_id} · {p.model || "nessun modello"}</option>)}
        </Select>
      </Field>
      {profile && (
        <div className="profile-card">
          <KeyValue items={[
            ["Runtime", <span key="r" className="row"><span className="mono">{runtime?.name ?? profile.runtime_id}</span>{runtime && <StatusPill status={runtime.status} />}</span>],
            ["Modello", profile.model ? <span className="mono">{profile.model}</span> : <Pill tone="warn">da scegliere</Pill>],
            ["Contesto", formatContext(profile.params.context)],
            ["Temperatura", profile.params.temperature ?? "—"],
          ]} />
          {profile.description && <p className="faint text-sm mt-3">{profile.description}</p>}
        </div>
      )}
      <p className="row text-sm">
        <Icon name="info" size={14} />
        <span className="muted">Modifichi i profili in</span> <Link to="/agents?tab=profiles">Profili di inferenza</Link>
        <span className="muted">· scarichi i modelli in</span> <Link to="/models">Modelli</Link>
      </p>
    </div>
  );
}
