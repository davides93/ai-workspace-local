import type { ToolInfo } from "../../../shared/api/types";
import { cx } from "../../../shared/lib/cx";
import { Pill, Toggle } from "../../../shared/ui";
import { shortToolName, TOOL_RISK } from "../model/tools";

/** One tool: name, what it does, its risk and the switch that gives or takes it from every agent. */
export function ToolRow({ tool, onToggle }: { tool: ToolInfo; onToggle: (enabled: boolean) => void }) {
  const risk = TOOL_RISK[tool.risk];
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
      <Toggle label={`Attiva ${tool.name}`} checked={tool.enabled} onChange={onToggle} />
    </li>
  );
}
