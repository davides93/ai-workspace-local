import { useEffect, useId, useRef, useState } from "react";
import { Chip, EmptyState, Icon, IconButton, Input, Pill, Toggle } from "../../../shared/ui";
import { RISK_META } from "../../../shared/lib/policyModes";
import type { PolicyMode, SandboxMode, ToolRisk } from "../../../shared/api/types";
import {
  blockedTools, groupState, groupTools, isAllTools, isSelected, orphanTools, setAllTools, setGroup, summarizeTools, toggleTool,
  TRAINING_TOOL, EVAL_TOOL, isExplicitTool, type PickerTool, type ToolGroup,
} from "../model/tools";

const RISKS: ToolRisk[] = ["read", "network", "write", "exec", "computer"];

/** Checkbox that can show the "some of them" state. */
function Check({ checked, indeterminate, disabled, onChange, label }: { checked: boolean; indeterminate?: boolean; disabled?: boolean; onChange: (on: boolean) => void; label: string }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = !!indeterminate;
  }, [indeterminate]);
  return <input ref={ref} type="checkbox" className="tool-check" aria-label={label} checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />;
}

function ToolRow({ tool, checked, locked, onToggle }: { tool: PickerTool; checked: boolean; locked: boolean; onToggle: (on: boolean) => void }) {
  const risk = RISK_META[tool.risk];
  return (
    <li className="tool-row" data-off={tool.disabled || tool.unavailable ? "true" : undefined}>
      <label>
        <Check checked={checked} disabled={locked} onChange={onToggle} label={tool.name} />
        <span className="tool-main">
          <span className="tool-name mono">{tool.name}</span>
          <Pill tone={risk.tone} title={risk.hint}>{risk.label}</Pill>
          {tool.disabled && <Chip title="Disattivato per tutti in «Tool, MCP e browser»">disattivato</Chip>}
          {tool.unavailable && <Chip title={tool.unavailable}>non disponibile</Chip>}
        </span>
        <span className="tool-desc">{tool.description}{tool.unavailable ? ` — ${tool.unavailable}` : ""}</span>
      </label>
    </li>
  );
}

function Group({ group, value, locked, forceOpen, onChange }: { group: ToolGroup; value: string[]; locked: boolean; forceOpen: boolean; onChange: (next: string[]) => void }) {
  const names = group.tools.map((t) => t.name);
  const state = groupState(value, names);
  const picked = names.filter((n) => isSelected(value, n)).length;
  // Big MCP servers start folded; a search opens everything that matches.
  const [open, setOpen] = useState(!group.id.startsWith("mcp:") || group.tools.length <= 8);
  const shown = open || forceOpen;
  const bodyId = useId();
  return (
    <div className="tool-group" data-open={shown}>
      <div className="tool-group-head">
        <button type="button" className="tool-group-toggle" aria-expanded={shown} aria-controls={bodyId} onClick={() => setOpen(!shown)}>
          <Icon name="down" size={14} className="chev" />
          <span className="tool-group-title">{group.label}</span>
          <span className="faint text-xs num">{picked}/{names.length}</span>
        </button>
        <label className="check-inline text-xs">
          <Check checked={state === "all"} indeterminate={state === "some"} disabled={locked} label={`Seleziona tutti: ${group.label}`} onChange={(on) => onChange(setGroup(value, names, on))} />
          Tutti
        </label>
      </div>
      {shown && (
        <ul id={bodyId} className="tool-list">
          {group.tools.map((t) => <ToolRow key={t.name} tool={t} checked={isSelected(value, t.name)} locked={locked} onToggle={(on) => onChange(toggleTool(value, t.name, on))} />)}
        </ul>
      )}
    </div>
  );
}

/**
 * Tools of an agent: all of them (`*`) or a hand-picked list, grouped by family, with search,
 * "select all" per group and the risk of each tool. The internal tools (plan, sub-agents, skills…)
 * are part of the list, so editing it never drops them by accident.
 */
export function ToolPicker({ value, onChange, catalog, mode, sandbox }: {
  value: string[];
  onChange: (next: string[]) => void;
  catalog: PickerTool[];
  mode: PolicyMode;
  sandbox: SandboxMode;
}) {
  const [query, setQuery] = useState("");
  const all = isAllTools(value);
  const ordinaryTools = catalog.filter((tool) => !isExplicitTool(tool.name));
  const training = catalog.find((tool) => tool.name === TRAINING_TOOL);
  const pageScripts = catalog.find((tool) => tool.name === EVAL_TOOL);
  const groups = groupTools(ordinaryTools, query);
  const summary = summarizeTools(value, catalog);
  const orphans = all ? [] : orphanTools(value, catalog);
  const blocked = blockedTools(value, catalog, mode, sandbox);

  return (
    <div className="tool-picker">
      <div className="tool-top">
        <div className="tool-all">
          <Toggle id="tools-all" label="Tutti i tool" checked={all} onChange={(on) => onChange([...setAllTools(value.filter((name) => !isExplicitTool(name)), on, ordinaryTools), ...value.filter(isExplicitTool)])} />
          <label htmlFor="tools-all"><b>Tutti i tool</b> <span className="muted text-sm">(<code>*</code>) anche quelli aggiunti in futuro, MCP compresi</span></label>
        </div>
        <div className="tool-summary" role="status">
          <span className="num"><b>{summary.count}</b> di {summary.total} tool</span>
          <span className="tool-risks">
            {RISKS.filter((r) => summary.byRisk[r]).map((r) => <Pill key={r} tone={RISK_META[r].tone}>{summary.byRisk[r]} {RISK_META[r].label}</Pill>)}
          </span>
        </div>
      </div>

      {all && <p className="tool-note text-sm"><Icon name="info" size={14} /> Con «Tutti i tool» l'agente usa i tool ordinari attivi. Disattiva l'interruttore per sceglierne alcuni: l'elenco parte da tutti, nulla va perso.</p>}

      {training && (
        <label className="row">
          <Toggle label="Consenti addestramento" checked={value.includes(TRAINING_TOOL)}
            onChange={(on) => onChange(on ? [...value, TRAINING_TOOL] : value.filter((name) => name !== TRAINING_TOOL))} />
          <span>Consenti addestramento <span className="muted text-sm">Dataset approvati, job e valutazione.</span></span>
        </label>
      )}

      {pageScripts && (
        <label className="row">
          <Toggle label="Consenti codice nella pagina" checked={value.includes(EVAL_TOOL)}
            onChange={(on) => onChange(on ? [...value, EVAL_TOOL] : value.filter((name) => name !== EVAL_TOOL))} />
          <span>Consenti codice nella pagina <span className="muted text-sm">Il browser esegue JavaScript scritto dall'agente, con la sessione e i cookie della pagina.</span></span>
        </label>
      )}

      <Input type="search" aria-label="Cerca un tool" className="input-search" placeholder="Cerca per nome, descrizione o gruppo…" value={query} onChange={(e) => setQuery(e.target.value)} />

      {groups.length === 0 ? (
        <EmptyState icon="search" title="Nessun tool corrisponde" description={`Nessun risultato per «${query}».`} />
      ) : (
        <div className="tool-groups">
          {groups.map((g) => <Group key={g.id} group={g} value={value} locked={all} forceOpen={query.trim() !== ""} onChange={onChange} />)}
        </div>
      )}

      {orphans.length > 0 && (
        <div className="tool-orphans" role="group" aria-label="Tool non più disponibili">
          <p className="text-sm"><b>Non più nel catalogo</b> <span className="muted">— un server MCP assente (non blocca il salvataggio) o un tool rinominato o inesistente (il backend lo rifiuta: toglilo dall'elenco).</span></p>
          <ul className="tool-list">
            {orphans.map((n) => (
              <li key={n} className="tool-row orphan">
                <span className="tool-name mono">{n}</span>
                <IconButton aria-label={`Rimuovi ${n} dall'elenco`} icon="close" size={14} onClick={() => onChange(toggleTool(value, n, false))} />
              </li>
            ))}
          </ul>
        </div>
      )}

      {blocked.length > 0 && (
        <p className="tool-warn text-sm" role="note">
          <Icon name="alert" size={14} /> {blocked.length === 1 ? "Un tool selezionato non potrà essere usato" : `${blocked.length} tool selezionati non potranno essere usati`}:
          {" "}{blocked.slice(0, 4).map((b) => `${b.name} (${b.why})`).join("; ")}{blocked.length > 4 ? "…" : ""}.
        </p>
      )}
    </div>
  );
}
