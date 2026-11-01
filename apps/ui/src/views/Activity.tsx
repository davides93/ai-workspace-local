import { useState } from "react";
import { useApp } from "../state/app";
import { useAsync } from "../components/ui";

const TONE: Record<string, string> = { allow: "ok", approved: "ok", ask: "warn", deny: "crit", rejected: "crit" };

export function ActivityView() {
  const { api } = useApp();
  const audit = useAsync(() => api.audit(), [api], 3000);
  const [actor, setActor] = useState<string>("all");
  const rows = (audit.data ?? []).filter((a) => actor === "all" || a.actor === actor);
  return (
    <div className="page">
      <div className="page-head">
        <div className="titles">
          <span className="eyebrow">Controllo</span>
          <h1>Attività</h1>
          <span className="muted">Audit trail di ogni decisione di policy, azione dei tool e comando di sistema.</span>
        </div>
        <div className="actions">
          <div className="seg" role="group" aria-label="Filtro attore">
            {[["all", "Tutto"], ["agent", "Agenti"], ["user", "Utente"], ["workflow", "Workflow"], ["system", "Sistema"]].map(([id, l]) => (
              <button key={id} aria-pressed={actor === id} onClick={() => setActor(id)}>{l}</button>
            ))}
          </div>
        </div>
      </div>
      <div className="table-wrap">
        <table className="data">
          <thead><tr><th>Quando</th><th>Attore</th><th>Azione</th><th>Decisione</th><th>Dettaglio</th><th>Run</th></tr></thead>
          <tbody>
            {rows.map((a) => (
              <tr key={a.id}>
                <td className="num mono" style={{ whiteSpace: "nowrap", fontSize: "var(--fs-xs)" }}>{new Date(a.ts).toLocaleString()}</td>
                <td>{a.actor}</td>
                <td className="mono">{a.tool ? `${a.action}:${a.tool}` : a.action}</td>
                <td>{a.decision ? <span className={`pill ${TONE[a.decision] ?? "neutral"}`}>{a.decision}</span> : "—"}</td>
                <td className="mono" style={{ fontSize: "var(--fs-xs)", maxWidth: 420, overflowWrap: "anywhere" }}>{a.detail}</td>
                <td className="mono faint" style={{ fontSize: "var(--fs-xs)" }}>{a.run_id ?? "—"}</td>
              </tr>
            ))}
            {rows.length === 0 && <tr><td colSpan={6} className="faint">Ancora nessuna attività.</td></tr>}
          </tbody>
        </table>
      </div>
    </div>
  );
}
