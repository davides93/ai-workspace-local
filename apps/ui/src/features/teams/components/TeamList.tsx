import { Link } from "../../../app/router";
import { Icon, Pill } from "../../../shared/ui";
import type { TeamDef } from "../../../shared/api/types";
import { NEW_TEAM, strategyInfo } from "../model/teams";

import { t as tr } from "../../../shared/i18n";
/** Teams as a list of links: the selected one lives in the URL (`#/teams/:teamId`). */
export function TeamList({ teams, selectedId }: { teams: TeamDef[]; selectedId: string | undefined }) {
  return (
    <div className="panel agent-list">
      <nav className="list" aria-label={tr("teams.team")}>
        {selectedId === NEW_TEAM && (
          <div className="list-item agent-item" aria-current="true">
            <Icon name="plus" />
            <div className="grow"><b>{tr("teams.nuovo_team")}</b><div className="faint text-xs">{tr("teams.non_ancora_salvato")}</div></div>
          </div>
        )}
        {teams.map((t) => (
          <Link key={t.id} to={`/teams/${encodeURIComponent(t.id)}`} className="list-item clickable agent-item" aria-current={t.id === selectedId}>
            <Icon name="team" />
            <div className="grow">
              <b>{t.name}</b>
              <div className="faint ellipsis text-xs">{t.description || `${t.members.length} ${t.members.length === 1 ? "membro" : "membri"}`}</div>
            </div>
            <Pill tone="neutral">{strategyInfo(t.strategy)?.label ?? t.strategy}</Pill>
          </Link>
        ))}
      </nav>
    </div>
  );
}
