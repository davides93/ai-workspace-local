import { useEffect, useMemo, useState } from "react";
import type { UseQueryResult } from "@tanstack/react-query";
import { Button, EmptyState, Icon, IconButton, QueryState, Select } from "../../../shared/ui";
import type { AgentProfile, Session, TeamDef } from "../../../shared/api/types";
import { useReflect } from "../../learning/api";
import { teamAgentId } from "../../teams/model/teams";
import { filterSessions, groupSessions } from "../model/sessions";
import { DeleteSessionDialog } from "./DeleteSessionDialog";
import { SessionItem } from "./SessionItem";

import { t as tr, t } from "../../../shared/i18n";
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
  const reflect = useReflect();

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
        <h2 className="grow">{tr("chat.sessioni")}</h2>
        {list && list.length > 0 && <span className="faint text-xs num" aria-label={tr("chat.sessioni_2", { length: list.length })}>{list.length}</span>}
        {onClose && <IconButton aria-label={tr("chat.chiudi_l_elenco_delle_sessioni")} icon="close" onClick={onClose} />}
      </div>

      <div className="ws-new">
        <div className="ws-new-row">
          <Select aria-label={tr("chat.agente_della_nuova_sessione")} value={agent} onChange={(e) => setAgent(e.target.value)} disabled={agents.length === 0}>
            {teams.length > 0 ? (
              <>
                <optgroup label={tr("chat.agenti")}>{agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}</optgroup>
                <optgroup label={tr("chat.team")}>{teams.map((t) => <option key={t.id} value={teamAgentId(t.id)}>{t.name}</option>)}</optgroup>
              </>
            ) : agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </Select>
          <Button variant="primary" icon="plus" disabled={!agent || creating} onClick={() => onCreate({ agent_id: agent, isolation: isolated ? "worktree" : "none" })}>
            {tr("chat.nuova")}</Button>
        </div>
        <label className="ws-check" title={tr("chat.l_agente_lavora_su_un")}>
          <input type="checkbox" checked={isolated} onChange={(e) => setIsolated(e.target.checked)} />
          <Icon name="branch" size={14} /> {" "}{t("chat.isola_in_un_worktree_git")}</label>
      </div>

      <div className="ws-search">
        <Icon name="search" size={15} />
        <input
          type="search"
          className="ws-search-input"
          aria-label={tr("chat.cerca_nelle_sessioni")}
          placeholder={tr("chat.cerca_per_titolo_o_agente")}
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
          empty={<EmptyState className="ws-sessions-empty" icon="chat" title={tr("chat.nessuna_sessione")} description={tr("chat.scegli_un_agente_qui_sopra")} />}
        >
          {() =>
            groups.length === 0 ? (
              <EmptyState
                className="ws-sessions-empty"
                icon="search"
                title={tr("chat.nessun_risultato")}
                description={<>{t("chat.nessuna_sessione_contiene", { query })}</>}
                action={<Button size="sm" onClick={() => setQuery("")}>{tr("chat.azzera_la_ricerca")}</Button>}
              />
            ) : (
              <nav aria-label={tr("chat.elenco_delle_sessioni")}>
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
                          reflecting={reflect.isPending && reflect.variables === s.id}
                          onSelect={() => onSelect(s.id)}
                          onRename={() => setRenamingId(s.id)}
                          onRenamed={() => setRenamingId(null)}
                          onReflect={() => reflect.mutate(s.id)}
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
