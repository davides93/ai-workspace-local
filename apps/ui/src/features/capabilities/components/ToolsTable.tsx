import { Chip, DataTable, Pill, QueryState, Toggle, type Column, type PillTone } from "../../../shared/ui";
import type { ToolInfo, ToolRisk } from "../../../shared/api/types";
import { useSetToolEnabled, useTools } from "../api";

export const RISK: Record<ToolRisk, { label: string; tone: PillTone }> = {
  read: { label: "lettura", tone: "ok" },
  write: { label: "scrittura", tone: "warn" },
  exec: { label: "esecuzione", tone: "crit" },
  network: { label: "rete", tone: "info" },
  computer: { label: "computer-use", tone: "crit" },
};

/** Browser and computer-use tools have their own tab. */
export const isBrowserTool = (t: ToolInfo) => t.name.startsWith("browser_") || t.name.startsWith("computer_");

export function ToolsTable({ filter, empty }: { filter?: (t: ToolInfo) => boolean; empty?: string }) {
  const tools = useTools();
  const setEnabled = useSetToolEnabled();
  const columns: Column<ToolInfo>[] = [
    { key: "name", header: "Tool", className: "mono", render: (t) => t.name },
    { key: "source", header: "Origine", render: (t) => <Chip>{t.source === "mcp" ? `mcp · ${t.server}` : "builtin"}</Chip> },
    { key: "risk", header: "Rischio", render: (t) => <Pill tone={RISK[t.risk].tone}>{RISK[t.risk].label}</Pill> },
    { key: "desc", header: "Descrizione", render: (t) => <>{t.description}{!t.available && <div className="faint text-xs">Non disponibile: {t.unavailable_reason}</div>}</> },
    { key: "on", header: "Attivo", render: (t) => <Toggle label={`Attiva ${t.name}`} checked={t.enabled} onChange={(enabled) => setEnabled.mutate({ name: t.name, enabled })} /> },
  ];
  return (
    <QueryState query={tools}>
      {(all) => <DataTable label="Tool disponibili" columns={columns} rows={filter ? all.filter(filter) : all} rowKey={(t) => t.name} empty={empty} />}
    </QueryState>
  );
}
