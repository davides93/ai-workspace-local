import { useState } from "react";
import type { McpServer, ToolInfo } from "../../../shared/api/types";
import { Chip, Chips, Icon, IconButton, Pill, StatusPill, Toggle } from "../../../shared/ui";
import { serverTarget } from "../model/mcpForm";
import { serverTools } from "../model/tools";
import { ToolRow } from "./ToolRow";

/** One MCP server: state and error, what it runs, its tools (expandable), switch, edit and remove. */
export function McpServerCard({ server, tools, onToggleServer, onToggleTool, onEdit, onRemove }: {
  server: McpServer;
  /** The whole tool registry: the server only reports names. */
  tools: ToolInfo[];
  onToggleServer: (enabled: boolean) => void;
  onToggleTool: (name: string, enabled: boolean) => void;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const [open, setOpen] = useState(false);
  const list = serverTools(server, tools);
  const listId = `mcp-tools-${server.id}`;
  const envNames = Object.keys(server.env ?? {});
  const headerNames = Object.keys(server.headers ?? {});

  return (
    <article className="cap-mcp" aria-label={`Server MCP ${server.name}`}>
      <div className="cap-mcp-head">
        <Icon name={server.transport === "http" ? "globe" : "terminal"} size={20} />
        <div className="grow">
          <div className="row">
            <b className="cap-mcp-title">{server.name}</b>
            <Pill>{server.transport === "http" ? "HTTP" : "stdio"}</Pill>
          </div>
          <div className="mono faint ellipsis text-xs" title={serverTarget(server)}>{serverTarget(server) || "—"}</div>
        </div>
        {server.enabled ? <StatusPill status={server.status} /> : <Pill>Disattivato</Pill>}
        <Toggle label={`Attiva il server ${server.name}`} checked={server.enabled} onChange={onToggleServer} />
        <IconButton aria-label={`Modifica ${server.name}`} icon="edit" size={16} onClick={onEdit} />
        <IconButton aria-label={`Rimuovi ${server.name}`} icon="trash" size={16} onClick={onRemove} />
      </div>

      {server.enabled && server.error && (
        <p className="cap-mcp-error"><Icon name="alert" size={14} /> <span>{server.error}</span></p>
      )}
      {(envNames.length > 0 || headerNames.length > 0) && (
        <Chips label="Variabili e header configurati">
          {envNames.map((n) => <Chip key={`e-${n}`} title="Variabile d'ambiente">env · {n}</Chip>)}
          {headerNames.map((n) => <Chip key={`h-${n}`} title="Header HTTP: il valore resta nascosto">header · {n}</Chip>)}
        </Chips>
      )}

      <button type="button" className="cap-mcp-tools-btn" aria-expanded={open} aria-controls={open ? listId : undefined} onClick={() => setOpen(!open)} disabled={list.length === 0}>
        <Icon name={open ? "down" : "forward"} size={14} />
        {list.length === 0 ? "Nessun tool esposto" : `${list.length} tool esposti`}
      </button>
      {open && list.length > 0 && (
        <ul className="cap-tools cap-tools-nested" id={listId}>
          {list.map((t) =>
            t.info ? (
              <ToolRow key={t.name} tool={t.info} onToggle={(enabled) => onToggleTool(t.info!.name, enabled)} />
            ) : (
              <li className="cap-tool" key={t.name}><div className="cap-tool-main"><code className="cap-tool-name">{t.name}</code></div></li>
            ),
          )}
        </ul>
      )}
    </article>
  );
}
