import { Link } from "../../../app/router";
import { EmptyState, Icon, Panel, Pill, QueryState, StatusPill } from "../../../shared/ui";
import { useRuntimes } from "../../runtimes/api";

/** Runtimes that are running (or starting) right now. */
export function RuntimesPanel() {
  const runtimes = useRuntimes();
  const live = (runtimes.data ?? []).filter((r) => r.status === "running" || r.status === "starting");
  return (
    <Panel title="Runtime attivi" icon="chip" actions={runtimes.data && <Pill tone={live.length ? "ok" : "neutral"}>{live.length}</Pill>}>
      <QueryState
        query={runtimes}
        isEmpty={() => live.length === 0}
        empty={<EmptyState icon="chip" title="Nessun runtime in esecuzione" description="Avviane uno per far lavorare gli agenti." action={<Link to="/runtimes" className="btn sm">Apri i runtime</Link>} />}
      >
        {() => (
          <div className="list list-flush">
            {live.map((r) => (
              <Link key={r.id} to={`/runtimes/${encodeURIComponent(r.id)}`} className="list-item clickable">
                <Icon name="chip" />
                <div className="grow">
                  <b className="block ellipsis">{r.name}</b>
                  <div className="faint mono ellipsis text-xs">{r.status_detail ?? r.endpoint}</div>
                </div>
                <StatusPill status={r.status} />
              </Link>
            ))}
          </div>
        )}
      </QueryState>
    </Panel>
  );
}
