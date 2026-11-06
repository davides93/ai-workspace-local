import { useState } from "react";
import { Field, Icon, Input, RowControls, Segmented, Select } from "../../../shared/ui";
import type { TeamDef } from "../../../shared/api/types";
import { agentLabel, isRemoteAgent, MAX_MEMBERS, STRATEGIES, type AgentOption, type TeamIssue } from "../model/teams";

const issueOf = (issues: TeamIssue[], field: TeamIssue["field"]) => issues.find((i) => i.field === field && i.level === "error")?.message;

/** One option list: the agents that are not taken yet, remote ones apart. */
function AgentSelect({ id, value, options, taken, onChange, placeholder, label }: {
  id: string; value: string; options: AgentOption[]; taken: readonly string[]; onChange: (id: string) => void; placeholder: string; label: string;
}) {
  const free = options.filter((o) => o.id === value || !taken.includes(o.id));
  const local = free.filter((o) => !o.remote);
  const remote = free.filter((o) => o.remote);
  return (
    <Select id={id} aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="">{placeholder}</option>
      {value && !options.some((o) => o.id === value) && <option value={value}>{value} (non trovato)</option>}
      {local.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}
      {remote.length > 0 && <optgroup label="Agenti remoti">{remote.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}</optgroup>}
    </Select>
  );
}

/** Strategy, then who coordinates (supervisor / aggregator) and the members with their roles. */
export function MembersSection({ team, options, issues, onChange }: {
  team: TeamDef;
  options: AgentOption[];
  issues: TeamIssue[];
  onChange: (patch: Partial<TeamDef>) => void;
}) {
  const [adding, setAdding] = useState("");
  const strategy = STRATEGIES.find((s) => s.id === team.strategy);
  const memberIds = team.members.map((m) => m.agent);
  const supervisorError = issueOf(issues, "supervisor");
  const membersError = issueOf(issues, "members");
  const move = (i: number, delta: -1 | 1) => {
    const next = team.members.slice();
    [next[i], next[i + delta]] = [next[i + delta], next[i]];
    onChange({ members: next });
  };
  const add = (agent: string) => {
    if (!agent) return;
    onChange({ members: [...team.members, { agent, role: "" }] });
    setAdding("");
  };
  return (
    <div className="form-grid">
      <div className="field">
        <span className="field-label" id="tm-strategy-label">Strategia</span>
        <Segmented
          label="Strategia del team" value={team.strategy}
          options={STRATEGIES.map((s) => ({ id: s.id, label: s.label, title: s.hint }))}
          onChange={(id) => onChange({ strategy: id })}
        />
        {strategy && <span className="hint">{strategy.hint}</span>}
      </div>

      {team.strategy === "supervisor" && (
        <Field label="Supervisore" htmlFor="tm-supervisor" error={supervisorError} hint="Il suo profilo guida il lavoro: i membri sono i suoi subagenti. Non può essere anche un membro.">
          <AgentSelect
            id="tm-supervisor" label="Supervisore" value={team.supervisor ?? ""} options={options} taken={memberIds} placeholder="Scegli l'agente…"
            onChange={(supervisor) => onChange({ supervisor: supervisor || undefined })}
          />
        </Field>
      )}

      <div className="field">
        <span className="field-label" id="tm-members-label">Membri <span className="faint num">({team.members.length}/{MAX_MEMBERS})</span></span>
        {team.members.length > 0 && (
          <ol className="team-members" aria-labelledby="tm-members-label">
            {team.members.map((m, i) => (
              <li key={m.agent} className="team-member">
                <span className="team-member-agent">
                  {team.strategy === "pipeline" && <span className="team-member-step" aria-hidden="true">{i + 1}.</span>}
                  <Icon name={isRemoteAgent(m.agent) ? "cloud" : "agent"} size={15} />
                  <span>{agentLabel(m.agent, options)}</span>
                </span>
                <Input
                  className="team-role" aria-label={`Ruolo di ${agentLabel(m.agent, options)}`} placeholder="Ruolo: che cosa fa nel team" value={m.role}
                  onChange={(e) => onChange({ members: team.members.map((x, j) => (j === i ? { ...x, role: e.target.value } : x)) })}
                />
                <RowControls
                  index={i} count={team.members.length} noun={`il membro ${agentLabel(m.agent, options)}`} onMove={(d) => move(i, d)}
                  onRemove={() => onChange({ members: team.members.filter((_, j) => j !== i) })}
                />
              </li>
            ))}
          </ol>
        )}
        {team.members.length < MAX_MEMBERS && (
          <div className="team-add">
            <AgentSelect
              id="tm-add" label="Aggiungi un membro" value={adding} options={options} taken={[...memberIds, ...(team.supervisor ? [team.supervisor] : [])]}
              placeholder="Aggiungi un membro…" onChange={add}
            />
          </div>
        )}
        {membersError && <span className="field-error" role="alert">{membersError}</span>}
        <span className="hint">
          {team.strategy === "pipeline" ? "I membri lavorano nell'ordine mostrato: ciascuno riceve la richiesta e il risultato del precedente." : "Il ruolo è la descrizione che il supervisore (o l'aggregatore) vede per ciascun membro."}
          {" "}Si possono usare anche agenti remoti, se registrati in Integrazioni.
        </span>
      </div>

      {team.strategy === "parallel" && (
        <Field label="Aggregatore (facoltativo)" htmlFor="tm-aggregator" error={issueOf(issues, "aggregator")} hint="Un agente che unisce i risultati dei membri in un'unica risposta. Senza, la risposta è l'elenco dei risultati con un'intestazione per ruolo.">
          <AgentSelect
            id="tm-aggregator" label="Aggregatore" value={team.aggregator ?? ""} options={options.filter((o) => !o.remote)} taken={[]} placeholder="Nessun aggregatore"
            onChange={(aggregator) => onChange({ aggregator: aggregator || undefined })}
          />
        </Field>
      )}
    </div>
  );
}
