import { useState } from "react";
import type { McpServer, ToolInfo } from "../../../shared/api/types";
import { Button, Chip, Chips, Icon, IconButton, Pill, StatusPill, Toggle } from "../../../shared/ui";
import { MCP_SANDBOX_LABEL, serverTarget } from "../model/mcpForm";
import { serverTools } from "../model/tools";
import { ToolRow } from "./ToolRow";

import { t as tr } from "../../../shared/i18n";
/** One MCP server: state and error, what it runs, its tools (expandable), switch, edit and remove. */
export function McpServerCard({ server, tools, onToggleServer, onToggleTool, onEdit, onRemove, onReconnect, onSignIn, onSignOut, onApprove, onIsolate, isolationAvailable, busy }: {
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
  /** «Consenti ed esegui» for a command that was never approved on this computer. */
  onApprove: () => void;
  /** «Isola»: switch a server that runs unisolated to workspace-write. */
  onIsolate: () => void;
  /** Can this system isolate a stdio server at all? (Not on Windows.) */
  isolationAvailable: boolean;
  busy?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const list = serverTools(server, tools);
  const listId = `mcp-tools-${server.id}`;
  const envNames = Object.keys(server.env ?? {});
  const headerNames = Object.keys(server.headers ?? {});
  const caps = server.status === "connected" ? server.capabilities : undefined;

  return (
    <article className="cap-mcp" aria-label={tr("capabilities.server_mcp_2", { name: server.name })}>
      <div className="cap-mcp-head">
        <Icon name={server.transport === "http" ? "globe" : "terminal"} size={20} />
        <div className="grow">
          <div className="row">
            <b className="cap-mcp-title">{server.name}</b>
            <Pill>{server.transport === "http" ? tr("capabilities.http") : tr("capabilities.stdio")}</Pill>
            {server.transport === "stdio" && (server.sandbox ?? "off") === "off" && (
              <Pill tone="warn" title="Il server gira con i tuoi permessi, senza isolamento">{tr("capabilities.non_isolato")}</Pill>
            )}
            {server.transport === "stdio" && server.sandbox && server.sandbox !== "off" && (
              <Pill tone="ok" title={`Isolato: ${MCP_SANDBOX_LABEL[server.sandbox]}${server.sandbox_network === false ? ", senza rete" : ""}`}>{tr("capabilities.isolato")}</Pill>
            )}
          </div>
          <div className="mono faint ellipsis text-xs" title={serverTarget(server)}>{serverTarget(server) || "—"}</div>
        </div>
        {server.enabled ? <StatusPill status={server.status} /> : <Pill>{tr("capabilities.disattivato")}</Pill>}
        {server.enabled && server.status === "reconnecting" && server.attempt ? <span className="faint text-xs">{tr("capabilities.tentativo", { attempt: server.attempt })}</span> : null}
        <Toggle label={tr("capabilities.attiva_il_server", { name: server.name })} checked={server.enabled} onChange={onToggleServer} />
        <IconButton aria-label={tr("capabilities.modifica", { name: server.name })} icon="edit" size={16} onClick={onEdit} />
        <IconButton aria-label={tr("capabilities.rimuovi", { name: server.name })} icon="trash" size={16} onClick={onRemove} />
      </div>

      {server.enabled && server.approval_required && (
        <div className="cap-note cap-note-warn text-sm" role="group" aria-label={tr("capabilities.approvazione_di", { name: server.name })}>
          <Icon name="alert" size={15} />
          <span className="grow">
            Questo server esegue il comando <code>{server.command_line ?? serverTarget(server)}</code> su questo computer. Non parte finché non lo consenti.
          </span>
          <Button size="sm" variant="primary" disabled={busy} onClick={onApprove}>{tr("capabilities.consenti_ed_esegui")}</Button>
        </div>
      )}
      {server.transport === "stdio" && (server.sandbox ?? "off") === "off" && (
        <div className="row text-sm muted" role="group" aria-label={tr("capabilities.isolamento_di", { name: server.name })}>
          <span className="grow">
            {isolationAvailable ? tr("capabilities.gira_senza_isolamento_con_l") : tr("capabilities.gira_senza_isolamento_su_questo")}
          </span>
          <Button size="sm" disabled={busy || !isolationAvailable} onClick={onIsolate}
            title={isolationAvailable ? "Passa a «Scrittura nel workspace», con rete" : "Non disponibile su questo sistema"}>{tr("capabilities.isola")}</Button>
        </div>
      )}
      {server.enabled && server.error && (
        <p className="cap-mcp-error"><Icon name="alert" size={14} /> <span>{server.error}</span></p>
      )}
      {server.enabled && (
        <div className="row" role="group" aria-label={tr("capabilities.connessione_di", { name: server.name })}>
          {caps && (
            <Chips label={tr("capabilities.cosa_offre_il_server")}>
              <Chip title="Strumenti (tools/list)">{tr("capabilities.strumenti", { p: caps.tools ? "" : " · no" })}</Chip>
              <Chip title="Risorse: due strumenti per elencarle e leggerle">{tr("capabilities.risorse", { p: caps.resources ? "" : " · no" })}</Chip>
              <Chip title="Prompt: li trovi nel campo del messaggio, in modalità Sviluppatore">{tr("capabilities.prompt", { p: caps.prompts ? "" : " · no" })}</Chip>
              {server.sampling && <Chip title="Il server può chiedere risposte al modello (con un tetto per esecuzione)">{tr("capabilities.campionamento_attivo")}</Chip>}
            </Chips>
          )}
          <span className="grow" />
          {server.transport === "http" && server.status === "auth_required" && (
            <Button size="sm" variant="primary" icon="key" disabled={busy} onClick={onSignIn}>{tr("capabilities.accedi")}</Button>
          )}
          {server.transport === "http" && server.signed_in && server.status !== "auth_required" && (
            <Button size="sm" disabled={busy} onClick={onSignOut} title="Dimentica l'accesso eseguito con OAuth">{tr("capabilities.esci")}</Button>
          )}
          {server.status !== "connected" && (
            <Button size="sm" icon="refresh" disabled={busy || server.status === "connecting"} onClick={onReconnect}>{tr("capabilities.riconnetti")}</Button>
          )}
        </div>
      )}
      {(envNames.length > 0 || headerNames.length > 0) && (
        <Chips label={tr("capabilities.variabili_e_header_configurati")}>
          {envNames.map((n) => <Chip key={`e-${n}`} title="Variabile d'ambiente">{tr("capabilities.env", { n })}</Chip>)}
          {headerNames.map((n) => <Chip key={`h-${n}`} title="Header HTTP: il valore resta nascosto">{tr("capabilities.header", { n })}</Chip>)}
        </Chips>
      )}

      <button type="button" className="cap-mcp-tools-btn" aria-expanded={open} aria-controls={open ? listId : undefined} onClick={() => setOpen(!open)} disabled={list.length === 0}>
        <Icon name={open ? "down" : "forward"} size={14} />
        {list.length === 0 ? tr("capabilities.nessun_tool_esposto") : tr("capabilities.tool_esposti", { length: list.length })}
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
