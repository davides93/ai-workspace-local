import { useState } from "react";
import { Button, EmptyState, Field, Icon, Input, Panel, Pill, QueryState, Select, Toggle } from "../../../shared/ui";
import { useSetToolsEnabled, useTools } from "../api";
import { countEnabled, groupState, groupTools, RISK_ORDER, TOOL_RISK, type RiskFilter } from "../model/tools";
import { ToolRow } from "./ToolRow";

const GROUP_ICON: Record<string, string> = {
  files: "file", shell: "terminal", web: "globe", browser: "globe", computer: "mouse", agent: "agent", other: "tool",
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
      empty={<EmptyState icon="tool" title="Nessun tool disponibile" description="Il workspace non ha ancora registrato tool: riavvia il server o aggiungi un server MCP." />}
    >
      {(all) => {
        const groups = groupTools(all, { query, risk });
        return (
          <>
            <div className="row filters">
              <Field label="Cerca" htmlFor="cap-search" className="grow cap-search">
                <Input id="cap-search" type="search" placeholder="Nome, descrizione o server" value={query} onChange={(e) => setQuery(e.target.value)} />
              </Field>
              <Field label="Rischio" htmlFor="tool-risk">
                <Select id="tool-risk" value={risk} onChange={(e) => setRisk(e.target.value as RiskFilter)}>
                  <option value="all">Tutti</option>
                  {RISK_ORDER.map((r) => <option key={r} value={r}>{TOOL_RISK[r].label}</option>)}
                </Select>
              </Field>
              <p className="muted text-sm cap-summary" aria-live="polite">{countEnabled(all)} di {all.length} tool attivi</p>
            </div>
            {groups.length === 0 && (
              <EmptyState
                icon="search" title="Nessun tool corrisponde" description="Prova con un altro termine o un altro livello di rischio."
                action={filtered && <Button onClick={() => { setQuery(""); setRisk("all"); }}>Azzera i filtri</Button>}
              />
            )}
            {groups.map((g) => {
              const state = groupState(g.tools);
              const icon = GROUP_ICON[g.id] ?? (g.id.startsWith("mcp:") ? "plug" : "tool");
              return (
                <Panel
                  key={g.id} padded={false} icon={icon}
                  title={<>{g.title} <Pill>{countEnabled(g.tools)}/{g.tools.length}</Pill></>}
                  actions={(
                    <span className="row nowrap cap-group-switch">
                      <span className="faint text-xs">{state === "all" ? "Tutti attivi" : state === "none" ? "Tutti spenti" : "Alcuni attivi"}</span>
                      <Toggle
                        label={`Attiva o disattiva tutti i tool di ${g.title}`} checked={state === "all"}
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
