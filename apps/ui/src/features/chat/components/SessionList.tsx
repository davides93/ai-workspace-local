import { useEffect, useMemo, useState } from "react";
import type { UseQueryResult } from "@tanstack/react-query";
import { Button, EmptyState, Icon, IconButton, QueryState, Select } from "../../../shared/ui";
import type { AgentProfile, Session, TeamDef } from "../../../shared/api/types";
import { teamAgentId } from "../../teams/model/teams";
import { filterSessions, groupSessions } from "../model/sessions";
import { DeleteSessionDialog } from "./DeleteSessionDialog";
import { SessionItem } from "./SessionItem";

export interface NewSessionInput {
  agent_id: string;
  isolation: "none" | "worktree";
}

/**
 * The sessions column: new session (agent, optional git worktree), search, and the sessions grouped
 * by day with rename and delete. The search box filters by title and agent name.
 */
export function SessionList({ sessions, agents, teams = [], defaultAgent, activeId, creating, onCreate, onSelect, onDeleted, onClose }: {
  sessions: UseQueryResult<Session[]>;
  agents: AgentProfile[];
  /** Teams are chosen like agents; their session agent id is `team:<id>`. */
  teams?: TeamDef[];
  defaultAgent: string;
  activeId: string | null;
  creating: boolean;
  onCreate: (input: NewSessionInput) => void;
  onSelect: (id: string) => void;
  onDeleted: (id: string) => void;
  /** Present when the list is shown off-canvas. */
  onClose?: () => void;
}) {
  const [agent, setAgent] = useState(defaultAgent);
  const [isolated, setIsolated] = useState(false);
  const [query, setQuery] = useState("");
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<Session | null>(null);

  // The agents arrive after the first render: preselect the default once they are known.
  useEffect(() => {
    if (!agents.some((a) => a.id === agent) && !teams.some((t) => teamAgentId(t.id) === agent)) setAgent(defaultAgent);
  }, [agents, teams, agent, defaultAgent]);

  const nameOf = useMemo(
    () => (id: string) => agents.find((a) => a.id === id)?.name ?? teams.find((t) => teamAgentId(t.id) === id)?.name,
    [agents, teams],
  );
  const agentName = nameOf;
  const list = sessions.data;
  const shown = useMemo(() => (list ? filterSessions(list, query, nameOf) : []), [list, query, nameOf]);
  const groups = useMemo(() => groupSessions(shown), [shown]);

  return (
    <div className="ws-sessions-in">
      <div className="ws-sessions-head">
        <h2 className="grow">Sessioni</h2>
        {list && list.length > 0 && <span className="faint text-xs num" aria-label={`${list.length} sessioni`}>{list.length}</span>}
        {onClose && <IconButton aria-label="Chiudi l'elenco delle sessioni" icon="close" onClick={onClose} />}
      </div>

      <div className="ws-new">
        <div className="ws-new-row">
          <Select aria-label="Agente della nuova sessione" value={agent} onChange={(e) => setAgent(e.target.value)} disabled={agents.length === 0}>
            {teams.length > 0 ? (
              <>
                <optgroup label="Agenti">{agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}</optgroup>
                <optgroup label="Team">{teams.map((t) => <option key={t.id} value={teamAgentId(t.id)}>{t.name}</option>)}</optgroup>
              </>
            ) : agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </Select>
          <Button variant="primary" icon="plus" disabled={!agent || creating} onClick={() => onCreate({ agent_id: agent, isolation: isolated ? "worktree" : "none" })}>
            Nuova
          </Button>
        </div>
        <label className="ws-check" title="L'agente lavora su un branch aiw/… in una copia separata del repository: rivedi le differenze e applicale quando vuoi.">
          <input type="checkbox" checked={isolated} onChange={(e) => setIsolated(e.target.checked)} />
          <Icon name="branch" size={14} /> Isola in un worktree git
        </label>
      </div>

      <div className="ws-search">
        <Icon name="search" size={15} />
        <input
          type="search"
          className="ws-search-input"
          aria-label="Cerca nelle sessioni"
          placeholder="Cerca per titolo o agente"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && query) {
              e.stopPropagation();
              setQuery("");
            }
          }}
        />
      </div>

      <div className="ws-sessions-scroll">
        <QueryState
          query={sessions}
          isEmpty={(l) => l.length === 0}
          empty={<EmptyState className="ws-sessions-empty" icon="chat" title="Nessuna sessione" description="Scegli un agente qui sopra e premi Nuova per iniziare." />}
        >
          {() =>
            groups.length === 0 ? (
              <EmptyState
                className="ws-sessions-empty"
                icon="search"
                title="Nessun risultato"
                description={<>Nessuna sessione contiene «{query}».</>}
                action={<Button size="sm" onClick={() => setQuery("")}>Azzera la ricerca</Button>}
              />
            ) : (
              <nav aria-label="Elenco delle sessioni">
                {groups.map((g) => (
                  <section key={g.id} aria-labelledby={`ws-group-${g.id}`}>
                    <h3 id={`ws-group-${g.id}`} className="ws-group">{g.label}</h3>
                    <ul className="ws-sessionlist">
                      {g.sessions.map((s) => (
                        <SessionItem
                          key={s.id}
                          session={s}
                          agentName={agentName(s.agent_id) ?? s.agent_id}
                          active={s.id === activeId}
                          renaming={renamingId === s.id}
                          onSelect={() => onSelect(s.id)}
                          onRename={() => setRenamingId(s.id)}
                          onRenamed={() => setRenamingId(null)}
                          onDelete={() => setDeleting(s)}
                        />
                      ))}
                    </ul>
                  </section>
                ))}
              </nav>
            )
          }
        </QueryState>
      </div>
      {deleting && <DeleteSessionDialog session={deleting} onClose={() => setDeleting(null)} onDeleted={onDeleted} />}
    </div>
  );
}
