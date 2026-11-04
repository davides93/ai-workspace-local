import { useMemo } from "react";
import { Link, navigate } from "../../../app/router";
import { defaultAgentId } from "../../../shared/lib/agents";
import { relTime } from "../../../shared/lib/format";
import { Button, EmptyState, Led, Panel, QueryState, type LedTone } from "../../../shared/ui";
import type { Session } from "../../../shared/api/types";
import { useAgents } from "../../agents/api";
import { useCreateSession, useSessions } from "../../chat/api";
import { useConfig } from "../../settings/api";

const ledTone = (s: Session): LedTone => (s.last_status === "failed" ? "crit" : s.last_status === "running" ? "run" : s.last_status === "cancelled" ? "warn" : s.last_status ? "ok" : "off");

/** The five most recently updated sessions. */
export function RecentSessions() {
  const sessions = useSessions();
  const agents = useAgents();
  const config = useConfig();
  const createSession = useCreateSession();
  const recent = useMemo(() => [...(sessions.data ?? [])].sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, 5), [sessions.data]);
  const agentId = defaultAgentId(agents.data ?? [], config.data?.effective["agent.default"]);
  return (
    <Panel title="Sessioni recenti" icon="chat" actions={<Link to="/chat" className="btn sm ghost">Apri il Workspace</Link>}>
      <QueryState
        query={sessions}
        isEmpty={(l) => l.length === 0}
        empty={
          <EmptyState
            icon="chat" title="Ancora nessuna sessione" description="Una sessione è una conversazione con un agente."
            action={<Button size="sm" variant="primary" icon="plus" disabled={!agentId || createSession.isPending} onClick={() => createSession.mutate({ agent_id: agentId }, { onSuccess: (s) => navigate(`/chat/${encodeURIComponent(s.id)}`) })}>Inizia una conversazione</Button>}
          />
        }
      >
        {() => (
          <div className="list list-flush">
            {recent.map((s) => (
              <Link key={s.id} to={`/chat/${encodeURIComponent(s.id)}`} className="list-item clickable">
                <Led tone={ledTone(s)} small />
                <div className="grow"><b className="ellipsis block">{s.title}</b></div>
                <time className="faint text-xs" dateTime={s.updated_at}>{relTime(s.updated_at)}</time>
              </Link>
            ))}
          </div>
        )}
      </QueryState>
    </Panel>
  );
}
