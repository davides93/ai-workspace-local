import { Field, Input } from "../../../shared/ui";
import type { BudgetLimits } from "../../../shared/api/types";

const FIELDS: { key: keyof BudgetLimits; id: string; label: string; hint: string }[] = [
  { key: "max_tokens", id: "ag-budget-tokens", label: "Token", hint: "Prompt e risposta di tutte le chiamate al modello." },
  { key: "max_turns", id: "ag-budget-turns", label: "Turni del modello", hint: "Contati sul run intero, subagenti compresi." },
  { key: "max_seconds", id: "ag-budget-seconds", label: "Secondi", hint: "Tempo massimo del run." },
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
      <p className="faint text-sm">Facoltativo. Quando un limite si esaurisce il run si chiude con un avviso invece di fallire. Un campo vuoto usa il valore delle Impostazioni (o nessun limite).</p>
      <div className="cols-2">
        {FIELDS.map((f, i) => (
          <Field key={f.key} label={f.label} htmlFor={f.id} hint={f.hint} error={i === 0 ? error : undefined}>
            <Input
              id={f.id} className="num" type="number" min={1} step={1} inputMode="numeric" placeholder="nessun limite"
              value={agent.budget?.[f.key] ?? ""} aria-invalid={!!error} onChange={(e) => set(f.key, e.target.value)}
            />
          </Field>
        ))}
      </div>
    </div>
  );
}
