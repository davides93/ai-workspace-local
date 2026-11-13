import { CopyButton, Field, Input, Textarea } from "../../../shared/ui";
import { formatNumber } from "../../../shared/lib/format";
import type { AgentProfile } from "../../../shared/api/types";
import { estimateTokens, LONG_PROMPT_CHARS, type AgentIssue } from "../model/agents";

import { t } from "../../../shared/i18n";
const issueOf = (issues: AgentIssue[], field: AgentIssue["field"]) => issues.find((i) => i.field === field && i.level === "error")?.message;
const warnOf = (issues: AgentIssue[], field: AgentIssue["field"]) => issues.find((i) => i.field === field && i.level === "warn")?.message;

/** Name, description and the system prompt (with a live count of characters and tokens). */
export function IdentitySection({ agent, issues, onChange, isNew }: {
  agent: AgentProfile;
  issues: AgentIssue[];
  onChange: (patch: Partial<AgentProfile>) => void;
  isNew: boolean;
}) {
  const chars = agent.system_prompt.length;
  const long = chars > LONG_PROMPT_CHARS;
  return (
    <div className="form-grid">
      <div className="cols-2">
        <Field label={t("agents.nome")} htmlFor="ag-name" error={issueOf(issues, "name")} hint={warnOf(issues, "name")}>
          <Input id="ag-name" value={agent.name} onChange={(e) => onChange({ name: e.target.value })} placeholder={t("agents.revisore_di_codice")} aria-invalid={!!issueOf(issues, "name")} autoComplete="off" />
        </Field>
        <Field label={t("agents.identificativo")} htmlFor="ag-id" hint={isNew ? t("agents.si_ricava_dal_nome_al") : t("agents.lo_usano_workflow_e_sessioni")}>
          <div className="copyfield-row">
            <Input id="ag-id" mono readOnly value={agent.id || "—"} />
            {agent.id && <CopyButton text={agent.id} />}
          </div>
        </Field>
      </div>
      <Field label={t("agents.descrizione")} htmlFor="ag-desc" hint={t("agents.compare_nell_elenco_e_come")}>
        <Input id="ag-desc" value={agent.description} onChange={(e) => onChange({ description: e.target.value })} placeholder={t("agents.che_cosa_sa_fare_in")} autoComplete="off" />
      </Field>
      <Field
        label={t("agents.istruzioni_di_sistema")} htmlFor="ag-prompt"
        hint={t("agents.si_aggiungono_ad_agents_md")}
      >
        <Textarea id="ag-prompt" rows={8} value={agent.system_prompt} onChange={(e) => onChange({ system_prompt: e.target.value })} spellCheck placeholder={t("agents.sei_un_spiega_ruolo_tono")} aria-describedby="ag-prompt-count" />
        <div id="ag-prompt-count" className={`prompt-count text-xs ${long ? "long" : "faint"}`} aria-live="polite">
          <span className="num">{t("agents.caratteri_circa_token", { p: formatNumber(chars, 0), p2: formatNumber(estimateTokens(agent.system_prompt), 0) })}</span>
          {long && <span> — {warnOf(issues, "system_prompt")}</span>}
        </div>
      </Field>
    </div>
  );
}
