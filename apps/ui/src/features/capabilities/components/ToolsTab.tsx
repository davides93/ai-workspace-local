import { useState } from "react";
import { Button, EmptyState, Field, Icon, Input, Panel, Pill, QueryState, Select, Toggle } from "../../../shared/ui";
import { useSetToolsEnabled, useTools } from "../api";
import { countEnabled, groupState, groupTools, isInternalTool, RISK_ORDER, switchableTools, TOOL_RISK, type RiskFilter } from "../model/tools";
import { ToolRow } from "./ToolRow";

import { t as tr } from "../../../shared/i18n";
const GROUP_ICON: Record<string, string> = {
  files: "file", shell: "terminal", web: "globe", browser: "globe", computer: "mouse", internal: "agent", other: "tool",
};

/** Every tool, grouped by what it does, with search, a risk filter and the on/off switches. */
export function ToolsTab() {
  const tools = useTools();
  const setEnabled = useSetToolsEnabled();
  const [query, setQuery] = useState("");
  const [risk, setRisk] = useState<RiskFilter>("all");
  const filtered = query.trim() !== "" || risk !== "all";

  return (
    <QueryState
      query={tools}
      isEmpty={(all) => all.length === 0}
      empty={<EmptyState icon="tool" title={tr("capabilities.nessun_tool_disponibile")} description={tr("capabilities.il_workspace_non_ha_ancora")} />}
    >
      {(all) => {
        const groups = groupTools(all, { query, risk });
        const switchable = switchableTools(all);
        const internalCount = all.length - switchable.length;
        return (
          <>
            <div className="row filters">
              <Field label={tr("capabilities.cerca")} htmlFor="cap-search" className="grow cap-search">
                <Input id="cap-search" type="search" placeholder={tr("capabilities.nome_descrizione_o_server")} value={query} onChange={(e) => setQuery(e.target.value)} />
              </Field>
              <Field label={tr("capabilities.rischio")} htmlFor="tool-risk">
                <Select id="tool-risk" value={risk} onChange={(e) => setRisk(e.target.value as RiskFilter)}>
                  <option value="all">{tr("capabilities.tutti")}</option>
                  {RISK_ORDER.map((r) => <option key={r} value={r}>{TOOL_RISK[r].label}</option>)}
                </Select>
              </Field>
              <p className="muted text-sm cap-summary" aria-live="polite">
                {tr("capabilities.di_tool_attivi", { p: countEnabled(switchable), length: switchable.length, p2: internalCount > 0 ? ` · ${internalCount} interni sempre attivi` : "" })}
              </p>
            </div>
            {groups.length === 0 && (
              <EmptyState
                icon="search" title={tr("capabilities.nessun_tool_corrisponde")} description={tr("capabilities.prova_con_un_altro_termine")}
                action={filtered && <Button onClick={() => { setQuery(""); setRisk("all"); }}>{tr("capabilities.azzera_i_filtri")}</Button>}
              />
            )}
            {groups.map((g) => {
              const state = groupState(g.tools);
              const internal = g.tools.every(isInternalTool);
              const icon = GROUP_ICON[g.id] ?? (g.id.startsWith("mcp:") ? "plug" : "tool");
              return (
                <Panel
                  key={g.id} padded={false} icon={icon}
                  title={<>{g.title} <Pill>{internal ? g.tools.length : `${countEnabled(g.tools)}/${g.tools.length}`}</Pill></>}
                  actions={internal ? <span className="faint text-xs">{tr("capabilities.sempre_attivi")}</span> : (
                    <span className="row nowrap cap-group-switch">
                      <span className="faint text-xs">{state === "all" ? tr("capabilities.tutti_attivi") : state === "none" ? tr("capabilities.tutti_spenti") : tr("capabilities.alcuni_attivi")}</span>
                      <Toggle
                        label={tr("capabilities.attiva_o_disattiva_tutti_i", { title: g.title })} checked={state === "all"}
                        onChange={(enabled) => setEnabled.mutate({ names: g.tools.map((t) => t.name), enabled })}
                      />
                    </span>
                  )}
                >
                  {g.hint && <p className="cap-group-hint faint text-xs"><Icon name="info" size={13} /> {g.hint}</p>}
                  <ul className="cap-tools">
                    {g.tools.map((t) => <ToolRow key={t.name} tool={t} onToggle={(enabled) => setEnabled.mutate({ names: [t.name], enabled })} />)}
                  </ul>
                </Panel>
              );
            })}
          </>
        );
      }}
    </QueryState>
  );
}
