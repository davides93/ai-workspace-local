import { Link } from "../../../app/router";
import { useUiMode } from "../../../app/useUiMode";
import { Field, Icon, KeyValue, Pill, Select, StatusPill } from "../../../shared/ui";
import { formatContext } from "../../../shared/lib/format";
import { useRecommend } from "../../models/api";
import { autoChoices, autoSummary, isAutoProfile } from "../../models/model/adviser";
import type { AgentProfile, InferenceProfile, RuntimeInfo } from "../../../shared/api/types";
import type { AgentIssue } from "../model/agents";

import { t } from "../../../shared/i18n";
/** Which inference profile the agent runs on, and what that profile means in practice. */
export function ModelSection({ agent, profiles, runtimes, issues, onChange }: {
  agent: AgentProfile;
  profiles: InferenceProfile[];
  runtimes: RuntimeInfo[];
  issues: AgentIssue[];
  onChange: (patch: Partial<AgentProfile>) => void;
}) {
  const [mode] = useUiMode();
  const recommend = useRecommend();
  const auto = isAutoProfile(agent.inference_profile);
  const profile = auto ? undefined : profiles.find((p) => p.id === agent.inference_profile);
  const runtime = runtimes.find((r) => r.id === profile?.runtime_id);
  const error = issues.find((i) => i.field === "inference_profile" && i.level === "error")?.message;
  const missing = !!agent.inference_profile && !auto && !profile && profiles.length > 0;
  return (
    <div className="form-grid">
      <Field label={t("agents.profilo_di_inferenza")} htmlFor="ag-profile" error={error} hint={t("agents.decide_runtime_modello_e_parametri")}>
        <Select id="ag-profile" value={agent.inference_profile} onChange={(e) => onChange({ inference_profile: e.target.value })} aria-invalid={!!error}>
          {missing && <option value={agent.inference_profile}>{t("agents.non_esiste_piu", { inference_profile: agent.inference_profile })}</option>}
          {!agent.inference_profile && <option value="">{t("agents.scegli_un_profilo")}</option>}
          <optgroup label={t("agents.scelta_automatica")}>
            {autoChoices(mode, agent.inference_profile).map((c) => <option key={c.value} value={c.value}>{c.label}</option>)}
          </optgroup>
          <optgroup label={t("agents.profili")}>
            {profiles.map((p) => <option key={p.id} value={p.id}>{p.name} — {p.runtime_id} · {p.model || t("agents.nessun_modello")}</option>)}
          </optgroup>
        </Select>
      </Field>
      {auto && <p className="faint text-sm">{autoSummary(agent.inference_profile, recommend.isError ? null : recommend.data)}</p>}
      {profile && (
        <div className="profile-card">
          <KeyValue items={[
            ["Runtime", <span key="r" className="row"><span className="mono">{runtime?.name ?? profile.runtime_id}</span>{runtime && <StatusPill status={runtime.status} />}</span>],
            ["Modello", profile.model ? <span className="mono">{profile.model}</span> : <Pill tone="warn">{t("agents.da_scegliere")}</Pill>],
            ["Contesto", formatContext(profile.params.context)],
            ["Temperatura", profile.params.temperature ?? "—"],
          ]} />
          {profile.description && <p className="faint text-sm mt-3">{profile.description}</p>}
        </div>
      )}
      <p className="row text-sm">
        <Icon name="info" size={14} />
        <span className="muted">{t("agents.modifichi_i_profili_in")}</span> <Link to="/models/profiles">{t("agents.profili_di_inferenza")}</Link>
        <span className="muted">{t("agents.scarichi_i_modelli_in")}</span> <Link to="/models">{t("agents.modelli")}</Link>
      </p>
    </div>
  );
}
