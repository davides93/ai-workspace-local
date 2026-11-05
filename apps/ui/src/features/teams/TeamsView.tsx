import { Link, useParams, useQueryParams } from "../../app/router";
import { EmptyState, Icon, MasterDetail, Page, PageHeader, QueryState } from "../../shared/ui";
import { useTeams } from "./api";
import { TeamEditor } from "./components/TeamEditor";
import { TeamList } from "./components/TeamList";
import "../agents/agents.css";
import "./teams.css";

/** `#/teams` and `#/teams/:teamId` (`new` opens a blank form, `?from=` a copy). */
export function TeamsView() {
  const teamId = useParams().teamId;
  const query = useQueryParams();
  const teams = useTeams();
  const list = teams.data ?? [];
  // Wide screens show the first team while the URL names none; on phones the list is the page.
  const selectedId = teamId ?? list[0]?.id;

  return (
    <Page className="agents-page">
      <PageHeader
        eyebrow="Lavoro" title="Team"
        subtitle="Un team riunisce più agenti: un supervisore che delega, membri che lavorano in parallelo o una catena di passi. Si usa in chat come un agente."
        actions={<Link to="/teams/new" className="btn primary"><Icon name="plus" size={15} /> Nuovo team</Link>}
      />
      <QueryState
        query={teams} isEmpty={(t) => t.length === 0 && !teamId}
        empty={<EmptyState icon="team" title="Nessun team" description="Crea il primo team: scegli la strategia e gli agenti che ne fanno parte." action={<Link to="/teams/new" className="btn primary">Nuovo team</Link>} />}
      >
        {(all) => (
          <MasterDetail
            className="agents-ws" detailOpen={!!teamId}
            list={<TeamList teams={all} selectedId={selectedId} />}
            detail={selectedId ? <TeamEditor teamId={selectedId} from={query.from} /> : null}
          />
        )}
      </QueryState>
    </Page>
  );
}
