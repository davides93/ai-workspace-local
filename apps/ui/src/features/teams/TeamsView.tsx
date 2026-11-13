import { Link, useParams, useQueryParams } from "../../app/router";
import { EmptyState, Icon, MasterDetail, Page, PageHeader, QueryState } from "../../shared/ui";
import { useTeams } from "./api";
import { TeamEditor } from "./components/TeamEditor";
import { TeamList } from "./components/TeamList";
import "../agents/agents.css";
import "./teams.css";

import { t as tr } from "../../shared/i18n";
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
        eyebrow={tr("nav.group_work")} title={tr("teams.team")}
        subtitle={tr("teams.un_team_riunisce_piu_agenti")}
        actions={<Link to="/teams/new" className="btn primary"><Icon name="plus" size={15} /> {tr("teams.nuovo_team")}</Link>}
      />
      <QueryState
        query={teams} isEmpty={(t) => t.length === 0 && !teamId}
        empty={<EmptyState icon="team" title={tr("teams.nessun_team")} description={tr("teams.crea_il_primo_team_scegli")} action={<Link to="/teams/new" className="btn primary">{tr("teams.nuovo_team")}</Link>} />}
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
