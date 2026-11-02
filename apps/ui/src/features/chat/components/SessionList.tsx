import { Icon, IconButton, Button, Led, type LedTone } from "../../../shared/ui";
import { relTime } from "../../../shared/lib/format";
import type { AgentProfile, Session } from "../../../shared/api/types";

const ledTone = (s: Session): LedTone => (s.last_status === "failed" ? "crit" : s.last_status === "running" ? "run" : s.last_status ? "ok" : "off");

function SessionItem({ s, agents, active, onClick }: { s: Session; agents: AgentProfile[]; active: boolean; onClick: () => void }) {
  const a = agents.find((x) => x.id === s.agent_id);
  return (
    <button type="button" className="session-item" aria-current={active} onClick={onClick}>
      <span className="t">{s.title}</span>
      <span className="m">
        <Led tone={ledTone(s)} small />
        {a?.name ?? s.agent_id} · {relTime(s.updated_at)}
        {s.worktree && <span title={s.worktree.branch} className="inline-flex"><Icon name="branch" size={12} /></span>}
      </span>
    </button>
  );
}

/** Left column of the chat: new session form and the list of sessions. */
export function SessionList({ sessions, agents, activeId, newAgent, onNewAgent, isolated, onIsolated, onCreate, onSelect, onClose, showClose }: {
  sessions: Session[];
  agents: AgentProfile[];
  activeId: string | null;
  newAgent: string;
  onNewAgent: (id: string) => void;
  isolated: boolean;
  onIsolated: (v: boolean) => void;
  onCreate: () => void;
  onSelect: (id: string) => void;
  onClose: () => void;
  showClose: boolean;
}) {
  return (
    <aside className="sessions" aria-label="Sessioni">
      <div className="sessions-head">
        <h2 className="grow">Sessioni</h2>
        {showClose && <IconButton aria-label="Chiudi" icon="close" onClick={onClose} />}
      </div>
      <div className="sessions-new">
        <select className="input" aria-label="Agente per la nuova sessione" value={newAgent} onChange={(e) => onNewAgent(e.target.value)}>
          {agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
        </select>
        <Button variant="primary" onClick={onCreate} title="Nuova sessione" aria-label="Nuova sessione" icon="plus" />
      </div>
      <label className="check-row" title="L'agente lavora su un branch aiw/… in una copia separata del repository: rivedi le differenze e applicale quando vuoi.">
        <input type="checkbox" checked={isolated} onChange={(e) => onIsolated(e.target.checked)} />
        <Icon name="branch" size={14} /> Isola in un worktree git
      </label>
      <div className="sessions-list">
        {sessions.map((s) => (
          <SessionItem key={s.id} s={s} agents={agents} active={s.id === activeId} onClick={() => onSelect(s.id)} />
        ))}
        {sessions.length === 0 && <div className="empty">Nessuna sessione. Scegli un agente e premi +.</div>}
      </div>
    </aside>
  );
}
