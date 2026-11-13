import { useState } from "react";
import { Field, Icon, Input, RowControls, Segmented, Select } from "../../../shared/ui";
import type { TeamDef } from "../../../shared/api/types";
import { agentLabel, isRemoteAgent, MAX_MEMBERS, STRATEGIES, type AgentOption, type TeamIssue } from "../model/teams";

import { t } from "../../../shared/i18n";
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
      {value && !options.some((o) => o.id === value) && <option value={value}>{t("teams.non_trovato", { value })}</option>}
      {local.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}
      {remote.length > 0 && <optgroup label={t("teams.agenti_remoti")}>{remote.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}</optgroup>}
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
        <span className="field-label" id="tm-strategy-label">{t("teams.strategia")}</span>
        <Segmented
          label={t("teams.strategia_del_team")} value={team.strategy}
          options={STRATEGIES.map((s) => ({ id: s.id, label: s.label, title: s.hint }))}
          onChange={(id) => onChange({ strategy: id })}
        />
        {strategy && <span className="hint">{strategy.hint}</span>}
      </div>

      {team.strategy === "supervisor" && (
        <Field label={t("teams.supervisore")} htmlFor="tm-supervisor" error={supervisorError} hint={t("teams.il_suo_profilo_guida_il")}>
          <AgentSelect
            id="tm-supervisor" label={t("teams.supervisore")} value={team.supervisor ?? ""} options={options} taken={memberIds} placeholder={t("teams.scegli_l_agente")}
            onChange={(supervisor) => onChange({ supervisor: supervisor || undefined })}
          />
        </Field>
      )}

      <div className="field">
        <span className="field-label" id="tm-members-label">{t("teams.membri")} <span className="faint num">({team.members.length}/{MAX_MEMBERS})</span></span>
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
                  className="team-role" aria-label={t("teams.ruolo_di", { agent: agentLabel(m.agent, options) })} placeholder={t("teams.ruolo_che_cosa_fa_nel")} value={m.role}
                  onChange={(e) => onChange({ members: team.members.map((x, j) => (j === i ? { ...x, role: e.target.value } : x)) })}
                />
                <RowControls
                  index={i} count={team.members.length} noun={t("teams.il_membro", { agent: agentLabel(m.agent, options) })} onMove={(d) => move(i, d)}
                  onRemove={() => onChange({ members: team.members.filter((_, j) => j !== i) })}
                />
              </li>
            ))}
          </ol>
        )}
        {team.members.length < MAX_MEMBERS && (
          <div className="team-add">
            <AgentSelect
              id="tm-add" label={t("teams.aggiungi_un_membro")} value={adding} options={options} taken={[...memberIds, ...(team.supervisor ? [team.supervisor] : [])]}
              placeholder={t("teams.aggiungi_un_membro_2")} onChange={add}
            />
          </div>
        )}
        {membersError && <span className="field-error" role="alert">{membersError}</span>}
        <span className="hint">
          {team.strategy === "pipeline" ? t("teams.members_hint_pipeline") : t("teams.members_hint_roles")}{" "}{t("teams.si_possono_usare_anche_agenti")}</span>
      </div>

      {team.strategy === "parallel" && (
        <Field label={t("teams.aggregatore_facoltativo")} htmlFor="tm-aggregator" error={issueOf(issues, "aggregator")} hint={t("teams.un_agente_che_unisce_i")}>
          <AgentSelect
            id="tm-aggregator" label={t("teams.aggregatore")} value={team.aggregator ?? ""} options={options.filter((o) => !o.remote)} taken={[]} placeholder={t("teams.nessun_aggregatore")}
            onChange={(aggregator) => onChange({ aggregator: aggregator || undefined })}
          />
        </Field>
      )}
    </div>
  );
}
