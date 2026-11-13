import { useEffect, useId, useRef, useState } from "react";
import { Chip, EmptyState, Icon, IconButton, Input, Pill, Toggle } from "../../../shared/ui";
import { RISK_META } from "../../../shared/lib/policyModes";
import type { PolicyMode, SandboxMode, ToolRisk } from "../../../shared/api/types";
import {
  blockedTools, groupState, groupTools, isAllTools, isSelected, orphanTools, setAllTools, setGroup, summarizeTools, toggleTool,
  TRAINING_TOOL, EVAL_TOOL, isExplicitTool, type PickerTool, type ToolGroup,
} from "../model/tools";

import { t as tr, tNode as trNode } from "../../../shared/i18n";
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
          {tool.disabled && <Chip title="Disattivato per tutti in «Tool, MCP e browser»">{tr("agents.disattivato")}</Chip>}
          {tool.unavailable && <Chip title={tool.unavailable}>{tr("agents.non_disponibile")}</Chip>}
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
          {tr("agents.tutti")}</label>
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
          <Toggle id="tools-all" label={tr("agents.tutti_i_tool")} checked={all} onChange={(on) => onChange([...setAllTools(value.filter((name) => !isExplicitTool(name)), on, ordinaryTools), ...value.filter(isExplicitTool)])} />
          <label htmlFor="tools-all"><b>{tr("agents.tutti_i_tool")}</b> <span className="muted text-sm">{trNode("agents.anche_quelli_aggiunti_in_futuro", { code: <code>*</code> })}</span></label>
        </div>
        <div className="tool-summary" role="status">
          <span className="num"><b>{summary.count}</b> di {summary.total} tool</span>
          <span className="tool-risks">
            {RISKS.filter((r) => summary.byRisk[r]).map((r) => <Pill key={r} tone={RISK_META[r].tone}>{summary.byRisk[r]} {RISK_META[r].label}</Pill>)}
          </span>
        </div>
      </div>

      {all && <p className="tool-note text-sm"><Icon name="info" size={14} /> {" "}{tr("agents.con_tutti_i_tool_l")}</p>}

      {training && (
        <label className="row">
          <Toggle label={tr("agents.consenti_addestramento")} checked={value.includes(TRAINING_TOOL)}
            onChange={(on) => onChange(on ? [...value, TRAINING_TOOL] : value.filter((name) => name !== TRAINING_TOOL))} />
          <span>{trNode("agents.consenti_addestramento_2", { span: <span className="muted text-sm">{tr("agents.dataset_approvati_job_e_valutazione")}</span> })}</span>
        </label>
      )}

      {pageScripts && (
        <label className="row">
          <Toggle label={tr("agents.consenti_codice_nella_pagina")} checked={value.includes(EVAL_TOOL)}
            onChange={(on) => onChange(on ? [...value, EVAL_TOOL] : value.filter((name) => name !== EVAL_TOOL))} />
          <span>{trNode("agents.consenti_codice_nella_pagina_2", { span: <span className="muted text-sm">{tr("agents.il_browser_esegue_javascript_scritto")}</span> })}</span>
        </label>
      )}

      <Input type="search" aria-label={tr("agents.cerca_un_tool")} className="input-search" placeholder={tr("agents.cerca_per_nome_descrizione_o")} value={query} onChange={(e) => setQuery(e.target.value)} />

      {groups.length === 0 ? (
        <EmptyState icon="search" title={tr("agents.nessun_tool_corrisponde")} description={tr("agents.nessun_risultato_per", { query })} />
      ) : (
        <div className="tool-groups">
          {groups.map((g) => <Group key={g.id} group={g} value={value} locked={all} forceOpen={query.trim() !== ""} onChange={onChange} />)}
        </div>
      )}

      {orphans.length > 0 && (
        <div className="tool-orphans" role="group" aria-label={tr("agents.tool_non_piu_disponibili")}>
          <p className="text-sm"><b>{tr("agents.non_piu_nel_catalogo")}</b> <span className="muted">{tr("agents.un_server_mcp_assente_non")}</span></p>
          <ul className="tool-list">
            {orphans.map((n) => (
              <li key={n} className="tool-row orphan">
                <span className="tool-name mono">{n}</span>
                <IconButton aria-label={tr("agents.rimuovi_dall_elenco", { n })} icon="close" size={14} onClick={() => onChange(toggleTool(value, n, false))} />
              </li>
            ))}
          </ul>
        </div>
      )}

      {blocked.length > 0 && (
        <p className="tool-warn text-sm" role="note">
          <Icon name="alert" size={14} /> {blocked.length === 1 ? tr("agents.un_tool_selezionato_non_potra") : tr("agents.tool_selezionati_non_potranno_essere", { length: blocked.length })}:
          {" "}{blocked.slice(0, 4).map((b) => `${b.name} (${b.why})`).join("; ")}{blocked.length > 4 ? "…" : ""}.
        </p>
      )}
    </div>
  );
}
