import { Field, Input } from "../../../shared/ui";
import type { BudgetLimits } from "../../../shared/api/types";

import { t, t as tr } from "../../../shared/i18n";
const FIELDS: { key: keyof BudgetLimits; id: string; label: string; hint: string }[] = [
  { key: "max_tokens", id: "ag-budget-tokens", get label() { return tr("agents.token"); }, get hint() { return tr("agents.prompt_e_risposta_di_tutte"); } },
  { key: "max_turns", id: "ag-budget-turns", get label() { return tr("agents.turni_del_modello"); }, get hint() { return tr("agents.contati_sul_run_intero_subagenti"); } },
  { key: "max_seconds", id: "ag-budget-seconds", get label() { return tr("agents.secondi"); }, get hint() { return tr("agents.tempo_massimo_del_run"); } },
];

/** What the section edits: an agent profile or a team. */
interface HasBudget {
  budget?: BudgetLimits;
}

/** Optional limits of one run, shared with the sub-agents it starts. An empty field means "the global setting". */
export function BudgetSection({ agent, issues, onChange }: {
  agent: HasBudget;
  issues: { field: string; level: string; message: string }[];
  onChange: (patch: HasBudget) => void;
}) {
  const error = issues.find((i) => i.field === "budget" && i.level === "error")?.message;
  const set = (key: keyof BudgetLimits, raw: string) => {
    const next: BudgetLimits = { ...agent.budget };
    if (raw === "") delete next[key];
    else next[key] = Number(raw);
    onChange({ budget: next });
  };
  return (
    <div className="form-grid">
      <p className="faint text-sm">{t("agents.facoltativo_quando_un_limite_si")}</p>
      <div className="cols-2">
        {FIELDS.map((f, i) => (
          <Field key={f.key} label={f.label} htmlFor={f.id} hint={f.hint} error={i === 0 ? error : undefined}>
            <Input
              id={f.id} className="num" type="number" min={1} step={1} inputMode="numeric" placeholder={t("agents.nessun_limite")}
              value={agent.budget?.[f.key] ?? ""} aria-invalid={!!error} onChange={(e) => set(f.key, e.target.value)}
            />
          </Field>
        ))}
      </div>
    </div>
  );
}
