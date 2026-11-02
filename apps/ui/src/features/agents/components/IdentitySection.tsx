import { CopyButton, Field, Input, Textarea } from "../../../shared/ui";
import { formatNumber } from "../../../shared/lib/format";
import type { AgentProfile } from "../../../shared/api/types";
import { estimateTokens, LONG_PROMPT_CHARS, type AgentIssue } from "../model/agents";

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
        <Field label="Nome" htmlFor="ag-name" error={issueOf(issues, "name")} hint={warnOf(issues, "name")}>
          <Input id="ag-name" value={agent.name} onChange={(e) => onChange({ name: e.target.value })} placeholder="Revisore di codice" aria-invalid={!!issueOf(issues, "name")} autoComplete="off" />
        </Field>
        <Field label="Identificativo" htmlFor="ag-id" hint={isNew ? "Si ricava dal nome al primo salvataggio." : "Lo usano workflow e sessioni: non cambia."}>
          <div className="copyfield-row">
            <Input id="ag-id" mono readOnly value={agent.id || "—"} />
            {agent.id && <CopyButton text={agent.id} />}
          </div>
        </Field>
      </div>
      <Field label="Descrizione" htmlFor="ag-desc" hint="Compare nell'elenco e come promemoria per gli altri agenti che possono delegargli un compito.">
        <Input id="ag-desc" value={agent.description} onChange={(e) => onChange({ description: e.target.value })} placeholder="Che cosa sa fare, in una riga" autoComplete="off" />
      </Field>
      <Field
        label="Istruzioni di sistema" htmlFor="ag-prompt"
        hint="Si aggiungono ad AGENTS.md del workspace e all'elenco delle skill disponibili."
      >
        <Textarea id="ag-prompt" rows={8} value={agent.system_prompt} onChange={(e) => onChange({ system_prompt: e.target.value })} spellCheck placeholder="Sei un… Spiega ruolo, tono e regole di lavoro." aria-describedby="ag-prompt-count" />
        <div id="ag-prompt-count" className={`prompt-count text-xs ${long ? "long" : "faint"}`} aria-live="polite">
          <span className="num">{formatNumber(chars, 0)} caratteri · circa {formatNumber(estimateTokens(agent.system_prompt), 0)} token</span>
          {long && <span> — {warnOf(issues, "system_prompt")}</span>}
        </div>
      </Field>
    </div>
  );
}
