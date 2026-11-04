import type { ToolInfo } from "../../../shared/api/types";
import { cx } from "../../../shared/lib/cx";
import { Chip, Pill, Toggle } from "../../../shared/ui";
import { isInternalTool, shortToolName, TOOL_RISK } from "../model/tools";

/**
 * One tool: name, what it does, its risk and the switch that gives or takes it from every agent. An internal tool
 * (of the agent loop) has no switch: it is always on, and says so with an "Interno" label.
 */
export function ToolRow({ tool, onToggle }: { tool: ToolInfo; onToggle: (enabled: boolean) => void }) {
  const risk = TOOL_RISK[tool.risk];
  const internal = isInternalTool(tool);
  return (
    <li className={cx("cap-tool", !tool.available && "is-unavailable", !tool.enabled && "is-off")}>
      <div className="cap-tool-main">
        <code className="cap-tool-name" title={tool.name}>{shortToolName(tool)}</code>
        {tool.description && <span className="cap-tool-desc">{tool.description}</span>}
        {!tool.available && (
          <span className="cap-tool-why">Non disponibile{tool.unavailable_reason ? `: ${tool.unavailable_reason}` : " su questo device"}</span>
        )}
      </div>
      <Pill tone={risk.tone} title={risk.hint}>{risk.label}</Pill>
      {internal
        ? <Chip className="cap-tool-internal" title="Implementato dal ciclo dell'agente: sempre attivo, non si può disattivare">Interno</Chip>
        : <Toggle label={`Attiva ${tool.name}`} checked={tool.enabled} onChange={onToggle} />}
    </li>
  );
}
