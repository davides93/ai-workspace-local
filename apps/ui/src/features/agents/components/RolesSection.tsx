import { Field, Select } from "../../../shared/ui";
import { autoChoices } from "../../models/model/adviser";
import type { AgentProfile, InferenceProfile, ModelRole } from "../../../shared/api/types";
import { MODES, ROLE_INFO, ROLE_KEYS, rolesMode, withRole, withRolesMode, type RolesMode } from "../model/roles";

interface Props {
  agent: AgentProfile;
  profiles: InferenceProfile[];
  onChange: (next: AgentProfile) => void;
}

/**
 * "Ruoli dei modelli" (developer mode only): one run can use several models: one that plans, one that
 * carries on, a final check and a small one for summaries. Automatico lets the app decide from the device.
 */
export function RolesSection({ agent, profiles, onChange }: Props) {
  const mode = rolesMode(agent.roles);
  const custom = typeof agent.roles === "object" ? agent.roles : {};
  return (
    <div className="form-grid">
      <fieldset className="tr-sources">
        <legend className="field-label">Come usare i modelli</legend>
        {MODES.map((m) => (
          <label key={m.value}>
            <input type="radio" name="ag-roles" value={m.value} checked={mode === m.value} onChange={() => onChange(withRolesMode(agent, m.value as RolesMode))} /> <b>{m.label}</b>
            <span className="faint text-xs"> — {m.hint}</span>
          </label>
        ))}
      </fieldset>
      {mode === "custom" && ROLE_KEYS.map((role: ModelRole) => (
        <Field key={role} label={ROLE_INFO[role].label} htmlFor={`ag-role-${role}`} hint={ROLE_INFO[role].hint}>
          <Select id={`ag-role-${role}`} value={custom[role] ?? ""} onChange={(e) => onChange(withRole(agent, role, e.target.value))}>
            <option value="">{ROLE_INFO[role].empty}</option>
            <optgroup label="Scelta automatica">
              {autoChoices("developer", custom[role]).map((c) => <option key={c.value} value={c.value}>{c.label}</option>)}
            </optgroup>
            <optgroup label="Profili">
              {profiles.map((p) => <option key={p.id} value={p.id}>{p.name} — {p.model || p.runtime_id}</option>)}
            </optgroup>
          </Select>
        </Field>
      ))}
      {mode !== "single" && (
        <p className="faint text-sm">
          Un secondo modello si carica solo se sta in memoria insieme al primo; se il motore regge un solo modello per volta, il run ne usa uno solo e lo scrive tra gli eventi.
        </p>
      )}
    </div>
  );
}
