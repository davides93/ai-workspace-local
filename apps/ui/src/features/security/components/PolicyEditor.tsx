import { useEffect, useState } from "react";
import { Button, EmptyState, Field, IconButton, Input, Pill, QueryState, Select, type PillTone } from "../../../shared/ui";
import type { PolicyAction, PolicyRule } from "../../../shared/api/types";
import { usePolicy, useSetPolicy } from "../api";

const ACTIONS: { id: PolicyAction; label: string; tone: PillTone }[] = [
  { id: "allow", label: "Consenti", tone: "ok" },
  { id: "ask", label: "Chiedi", tone: "warn" },
  { id: "deny", label: "Nega", tone: "crit" },
];

const sameRules = (a: PolicyRule[], b: PolicyRule[]) => JSON.stringify(a) === JSON.stringify(b);

/** Rules `tool(pattern) → allow | ask | deny`, evaluated on top of the approval mode of the agent. */
export function PolicyEditor() {
  const policy = usePolicy();
  const save = useSetPolicy();
  const [rules, setRules] = useState<PolicyRule[]>([]);
  const [draft, setDraft] = useState<PolicyRule>({ tool: "", pattern: "", action: "ask" });
  const saved = policy.data?.rules;

  useEffect(() => {
    if (saved) setRules(structuredClone(saved));
  }, [saved]);

  const dirty = !!saved && !sameRules(saved, rules);
  const add = () => {
    if (!draft.tool.trim()) return;
    setRules([...rules, { tool: draft.tool.trim(), pattern: draft.pattern?.trim() || null, action: draft.action }]);
    setDraft({ tool: "", pattern: "", action: draft.action });
  };

  return (
    <QueryState query={policy}>
      {() => (
        <div className="stack stack-lg">
          <div className="table-wrap">
            <table className="data" aria-label="Regole di policy">
              <thead><tr><th>Tool</th><th>Pattern</th><th>Azione</th><th><span className="sr-only">Rimuovi</span></th></tr></thead>
              <tbody>
                {rules.map((r, i) => (
                  <tr key={`${r.tool}-${r.pattern}-${i}`}>
                    <td className="mono">{r.tool}</td>
                    <td className="mono">{r.pattern || <span className="faint">qualsiasi</span>}</td>
                    <td><Pill tone={ACTIONS.find((a) => a.id === r.action)?.tone}>{ACTIONS.find((a) => a.id === r.action)?.label ?? r.action}</Pill></td>
                    <td><IconButton aria-label={`Rimuovi la regola ${r.tool}`} icon="trash" size={16} onClick={() => setRules(rules.filter((_, j) => j !== i))} /></td>
                  </tr>
                ))}
                {rules.length === 0 && <tr><td colSpan={4}><EmptyState icon="shield" title="Nessuna regola" description="Senza regole decide solo la modalità di approvazione dell'agente." /></td></tr>}
              </tbody>
            </table>
          </div>

          <form className="row form-inline" onSubmit={(e) => { e.preventDefault(); add(); }}>
            <Field label="Tool (glob)" htmlFor="pol-tool" className="grow">
              <Input id="pol-tool" mono placeholder="shell, mcp__github__*" value={draft.tool} onChange={(e) => setDraft({ ...draft, tool: e.target.value })} />
            </Field>
            <Field label="Pattern (opzionale)" htmlFor="pol-pattern" className="grow">
              <Input id="pol-pattern" mono placeholder="git status*, ~/Workspace/**" value={draft.pattern ?? ""} onChange={(e) => setDraft({ ...draft, pattern: e.target.value })} />
            </Field>
            <Field label="Azione" htmlFor="pol-action">
              <Select id="pol-action" value={draft.action} onChange={(e) => setDraft({ ...draft, action: e.target.value as PolicyAction })}>
                {ACTIONS.map((a) => <option key={a.id} value={a.id}>{a.label}</option>)}
              </Select>
            </Field>
            <Button type="submit" icon="plus" disabled={!draft.tool.trim()}>Aggiungi</Button>
          </form>

          <div className="row">
            <Button variant="primary" onClick={() => save.mutate(rules)} disabled={!dirty || save.isPending}>Salva regole</Button>
            {dirty && <Button variant="ghost" onClick={() => saved && setRules(structuredClone(saved))}>Annulla modifiche</Button>}
            {dirty && <span className="faint text-sm">Modifiche non salvate</span>}
          </div>
        </div>
      )}
    </QueryState>
  );
}
