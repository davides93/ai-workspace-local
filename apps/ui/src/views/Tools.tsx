import { useState } from "react";
import { useApp } from "../state/app";
import { Icon, StatusPill, useAsync } from "../components/ui";
import type { ToolRisk } from "../api/types";

const RISK: Record<ToolRisk, { label: string; tone: string }> = {
  read: { label: "lettura", tone: "ok" },
  write: { label: "scrittura", tone: "warn" },
  exec: { label: "esecuzione", tone: "crit" },
  network: { label: "rete", tone: "info" },
  computer: { label: "computer-use", tone: "crit" },
};

export function ToolsView() {
  const { api, toast } = useApp();
  const tools = useAsync(() => api.tools(), [api]);
  const servers = useAsync(() => api.mcpServers(), [api]);
  const [form, setForm] = useState({ name: "", transport: "stdio" as "stdio" | "http", command: "", url: "" });

  const add = async () => {
    if (!form.name) return;
    try {
      const parts = form.command.trim().split(/\s+/).filter(Boolean);
      await api.saveMcpServer(form.transport === "stdio"
        ? { name: form.name, transport: "stdio", command: parts[0], args: parts.slice(1) }
        : { name: form.name, transport: "http", url: form.url });
      toast(`Server MCP "${form.name}" aggiunto`);
      setForm({ name: "", transport: "stdio", command: "", url: "" });
      servers.reload();
      tools.reload();
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <div className="titles">
          <span className="eyebrow">Controllo</span>
          <h1>Tool e MCP</h1>
          <span className="muted">Cosa possono fare gli agenti. Il rischio decide quando serve una conferma.</span>
        </div>
      </div>

      <section className="section">
        <div className="section-head"><h2>Server MCP</h2><span className="muted">stdio (solo desktop) o streamable HTTP; i loro tool diventano <code>mcp__server__tool</code>.</span></div>
        <div className="panel">
          <div className="list">
            {servers.data?.map((s) => (
              <div className="list-item" key={s.id}>
                <Icon name="link" />
                <div className="grow">
                  <b>{s.name}</b>
                  <div className="mono faint ellipsis" style={{ fontSize: "var(--fs-xs)" }}>{s.transport === "stdio" ? `${s.command} ${(s.args ?? []).join(" ")}` : s.url}</div>
                </div>
                <span className="faint" style={{ fontSize: "var(--fs-xs)" }}>{s.tools.length} tool</span>
                <StatusPill status={s.status} />
                <button className="icon-btn" aria-label={`Rimuovi ${s.name}`} onClick={async () => { await api.removeMcpServer(s.id); servers.reload(); }}><Icon name="trash" size={16} /></button>
              </div>
            ))}
          </div>
          <div className="panel-body row" style={{ borderTop: "1px solid var(--line)", alignItems: "flex-end" }}>
            <div className="field" style={{ width: 150 }}>
              <label htmlFor="mcp-name">Nome</label>
              <input id="mcp-name" className="input" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="filesystem" />
            </div>
            <div className="field" style={{ width: 130 }}>
              <label htmlFor="mcp-transport">Trasporto</label>
              <select id="mcp-transport" className="input" value={form.transport} onChange={(e) => setForm({ ...form, transport: e.target.value as "stdio" | "http" })}>
                <option value="stdio">stdio</option>
                <option value="http">HTTP</option>
              </select>
            </div>
            <div className="field grow">
              <label htmlFor="mcp-target">{form.transport === "stdio" ? "Comando" : "URL"}</label>
              <input id="mcp-target" className="input mono" value={form.transport === "stdio" ? form.command : form.url}
                onChange={(e) => setForm(form.transport === "stdio" ? { ...form, command: e.target.value } : { ...form, url: e.target.value })}
                placeholder={form.transport === "stdio" ? "npx -y @modelcontextprotocol/server-filesystem ~/Workspace" : "http://127.0.0.1:8931/mcp"} />
            </div>
            <button className="btn primary" onClick={add}><Icon name="plus" size={15} /> Aggiungi</button>
          </div>
        </div>
      </section>

      <section className="section">
        <div className="section-head"><h2>Tool disponibili</h2><span className="muted">Disattiva un tool per toglierlo a tutti gli agenti.</span></div>
        <div className="table-wrap">
          <table className="data">
            <thead><tr><th>Tool</th><th>Origine</th><th>Rischio</th><th>Descrizione</th><th>Attivo</th></tr></thead>
            <tbody>
              {tools.data?.map((t) => (
                <tr key={t.name}>
                  <td className="mono">{t.name}</td>
                  <td>{t.source === "mcp" ? <span className="chip">mcp · {t.server}</span> : <span className="chip">builtin</span>}</td>
                  <td><span className={`pill ${RISK[t.risk].tone}`}>{RISK[t.risk].label}</span></td>
                  <td>{t.description}{!t.available && <div className="faint" style={{ fontSize: "var(--fs-xs)" }}>Non disponibile: {t.unavailable_reason}</div>}</td>
                  <td><span className="toggle"><input type="checkbox" aria-label={`Attiva ${t.name}`} checked={t.enabled} onChange={async (e) => { await api.setToolEnabled(t.name, e.target.checked); tools.reload(); }} /></span></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
