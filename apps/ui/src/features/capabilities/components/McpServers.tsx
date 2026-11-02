import { useState } from "react";
import { Button, Icon, IconButton, Panel, QueryState, StatusPill } from "../../../shared/ui";
import { useMcpServers, useRemoveMcpServer, useSaveMcpServer } from "../api";

type Transport = "stdio" | "http";

export function McpServers() {
  const servers = useMcpServers();
  const save = useSaveMcpServer();
  const remove = useRemoveMcpServer();
  const [form, setForm] = useState({ name: "", transport: "stdio" as Transport, command: "", url: "" });

  const add = () => {
    if (!form.name) return;
    const parts = form.command.trim().split(/\s+/).filter(Boolean);
    save.mutate(
      form.transport === "stdio"
        ? { name: form.name, transport: "stdio", command: parts[0], args: parts.slice(1) }
        : { name: form.name, transport: "http", url: form.url },
      { onSuccess: () => setForm({ name: "", transport: "stdio", command: "", url: "" }) },
    );
  };

  return (
    <Panel padded={false}>
      <QueryState query={servers}>
        {(list) => (
          <div className="list">
            {list.map((s) => (
              <div className="list-item" key={s.id}>
                <Icon name="link" />
                <div className="grow">
                  <b>{s.name}</b>
                  <div className="mono faint ellipsis text-xs">{s.transport === "stdio" ? `${s.command} ${(s.args ?? []).join(" ")}` : s.url}</div>
                  {s.error && <div className="text-xs crit-text">{s.error}</div>}
                </div>
                <span className="faint text-xs">{s.tools.length} tool</span>
                <StatusPill status={s.status} />
                <IconButton aria-label={`Rimuovi ${s.name}`} icon="trash" size={16} onClick={() => remove.mutate(s.id)} />
              </div>
            ))}
            {list.length === 0 && <div className="list-item faint">Nessun server MCP configurato.</div>}
          </div>
        )}
      </QueryState>
      <form className="panel-body row mcp-form" onSubmit={(e) => { e.preventDefault(); add(); }}>
        <div className="field mcp-name">
          <label htmlFor="mcp-name">Nome</label>
          <input id="mcp-name" className="input" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="filesystem" />
        </div>
        <div className="field mcp-transport">
          <label htmlFor="mcp-transport">Trasporto</label>
          <select id="mcp-transport" className="input" value={form.transport} onChange={(e) => setForm({ ...form, transport: e.target.value as Transport })}>
            <option value="stdio">stdio</option>
            <option value="http">HTTP</option>
          </select>
        </div>
        <div className="field grow mcp-target">
          <label htmlFor="mcp-target">{form.transport === "stdio" ? "Comando" : "URL"}</label>
          <input id="mcp-target" className="input mono" value={form.transport === "stdio" ? form.command : form.url}
            onChange={(e) => setForm(form.transport === "stdio" ? { ...form, command: e.target.value } : { ...form, url: e.target.value })}
            placeholder={form.transport === "stdio" ? "npx -y @modelcontextprotocol/server-filesystem ~/Workspace" : "http://127.0.0.1:8931/mcp"} />
        </div>
        <Button variant="primary" type="submit" icon="plus" disabled={save.isPending}>Aggiungi</Button>
      </form>
    </Panel>
  );
}
