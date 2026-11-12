import { useState } from "react";
import type { McpServer, ToolInfo } from "../../../shared/api/types";
import { Button, Chip, Chips, Icon, IconButton, Pill, StatusPill, Toggle } from "../../../shared/ui";
import { serverTarget } from "../model/mcpForm";
import { serverTools } from "../model/tools";
import { ToolRow } from "./ToolRow";

/** One MCP server: state and error, what it runs, its tools (expandable), switch, edit and remove. */
export function McpServerCard({ server, tools, onToggleServer, onToggleTool, onEdit, onRemove, onReconnect, onSignIn, onSignOut, busy }: {
  server: McpServer;
  /** The whole tool registry: the server only reports names. */
  tools: ToolInfo[];
  onToggleServer: (enabled: boolean) => void;
  onToggleTool: (name: string, enabled: boolean) => void;
  onEdit: () => void;
  onRemove: () => void;
  /** «Riconnetti»: a new attempt now. */
  onReconnect: () => void;
  /** «Accedi» (OAuth) and «Esci». */
  onSignIn: () => void;
  onSignOut: () => void;
  busy?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const list = serverTools(server, tools);
  const listId = `mcp-tools-${server.id}`;
  const envNames = Object.keys(server.env ?? {});
  const headerNames = Object.keys(server.headers ?? {});
  const caps = server.status === "connected" ? server.capabilities : undefined;

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
        {server.enabled && server.status === "reconnecting" && server.attempt ? <span className="faint text-xs">tentativo {server.attempt}</span> : null}
        <Toggle label={`Attiva il server ${server.name}`} checked={server.enabled} onChange={onToggleServer} />
        <IconButton aria-label={`Modifica ${server.name}`} icon="edit" size={16} onClick={onEdit} />
        <IconButton aria-label={`Rimuovi ${server.name}`} icon="trash" size={16} onClick={onRemove} />
      </div>

      {server.enabled && server.error && (
        <p className="cap-mcp-error"><Icon name="alert" size={14} /> <span>{server.error}</span></p>
      )}
      {server.enabled && (
        <div className="row" role="group" aria-label={`Connessione di ${server.name}`}>
          {caps && (
            <Chips label="Cosa offre il server">
              <Chip title="Strumenti (tools/list)">strumenti{caps.tools ? "" : " · no"}</Chip>
              <Chip title="Risorse: due strumenti per elencarle e leggerle">risorse{caps.resources ? "" : " · no"}</Chip>
              <Chip title="Prompt: li trovi nel campo del messaggio, in modalità Sviluppatore">prompt{caps.prompts ? "" : " · no"}</Chip>
              {server.sampling && <Chip title="Il server può chiedere risposte al modello (con un tetto per esecuzione)">campionamento attivo</Chip>}
            </Chips>
          )}
          <span className="grow" />
          {server.transport === "http" && server.status === "auth_required" && (
            <Button size="sm" variant="primary" icon="key" disabled={busy} onClick={onSignIn}>Accedi</Button>
          )}
          {server.transport === "http" && server.signed_in && server.status !== "auth_required" && (
            <Button size="sm" disabled={busy} onClick={onSignOut} title="Dimentica l'accesso eseguito con OAuth">Esci</Button>
          )}
          {server.status !== "connected" && (
            <Button size="sm" icon="refresh" disabled={busy || server.status === "connecting"} onClick={onReconnect}>Riconnetti</Button>
          )}
        </div>
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
