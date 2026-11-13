import { useState } from "react";
import { Link } from "../../../app/router";
import { Icon, Input, Pill } from "../../../shared/ui";
import { approvalMode } from "../../../shared/lib/policyModes";
import type { AgentProfile } from "../../../shared/api/types";
import { isBuiltinAgent, NEW_AGENT } from "../model/agents";

import { t } from "../../../shared/i18n";
const SEARCH_FROM = 7;

/** Agents as a list of links: the selected one lives in the URL (`#/agents/:agentId`). */
export function AgentList({ agents, selectedId }: { agents: AgentProfile[]; selectedId: string | undefined }) {
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const shown = q ? agents.filter((a) => `${a.name} ${a.description} ${a.id}`.toLowerCase().includes(q)) : agents;
  return (
    <div className="panel agent-list">
      {agents.length >= SEARCH_FROM && (
        <div className="agent-search">
          <Input type="search" aria-label={t("agents.cerca_un_agente")} placeholder={t("agents.cerca_un_agente_2")} value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
      )}
      <nav className="list" aria-label={t("agents.agenti")}>
        {selectedId === NEW_AGENT && (
          <div className="list-item agent-item" aria-current="true">
            <Icon name="plus" />
            <div className="grow"><b>{t("agents.nuovo_agente")}</b><div className="faint text-xs">{t("agents.non_ancora_salvato")}</div></div>
          </div>
        )}
        {shown.map((a) => {
          const mode = approvalMode(a.mode);
          return (
            <Link key={a.id} to={`/agents/${encodeURIComponent(a.id)}`} className="list-item clickable agent-item" aria-current={a.id === selectedId}>
              <Icon name="agent" />
              <div className="grow">
                <b>{a.name}</b>
                {isBuiltinAgent(a.id) && <span className="builtin-mark" title={t("agents.agente_integrato_si_puo_modificare")}><Icon name="lock" size={12} /><span className="sr-only">{t("agents.integrato")}</span></span>}
                <div className="faint ellipsis text-xs">{a.description || t("agents.nessuna_descrizione")}</div>
              </div>
              <Pill tone={mode?.tone ?? "neutral"}>{mode?.label ?? a.mode}</Pill>
            </Link>
          );
        })}
        {shown.length === 0 && <div className="list-item faint">{t("agents.nessun_agente_corrisponde")}</div>}
      </nav>
    </div>
  );
}
