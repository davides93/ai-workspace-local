import { Link, navigate, useParams, useQueryParams } from "../../app/router";
import { EmptyState, Icon, MasterDetail, Page, PageHeader, QueryState, TabPanel, Tabs } from "../../shared/ui";
import { useAgents } from "./api";
import { AgentEditor } from "./components/AgentEditor";
import { AgentList } from "./components/AgentList";
import { ProfilesPanel } from "./components/ProfilesPanel";
import "./agents.css";

import { t as tr } from "../../shared/i18n";
type Tab = "agents" | "profiles";

/** `#/agents` and `#/agents/:agentId` (`new` opens a blank form, `?from=` a copy); `?tab=profiles` lists the inference profiles. */
export function AgentsView() {
  const agentId = useParams().agentId;
  const query = useQueryParams();
  const agents = useAgents();
  const tab: Tab = query.tab === "profiles" && !agentId ? "profiles" : "agents";
  const list = agents.data ?? [];
  // Wide screens show the first agent while the URL names none; on phones the list is the page.
  const selectedId = agentId ?? list[0]?.id;

  return (
    <Page className="agents-page">
      <PageHeader
        eyebrow="Lavoro" title={tr("agents.agenti")}
        subtitle={tr("agents.un_agente_unisce_un_profilo")}
        actions={tab === "agents" && <Link to="/agents/new" className="btn primary"><Icon name="plus" size={15} /> {" "}{tr("agents.nuovo_agente")}</Link>}
      />
      <div className="agents-tabs" data-detail={agentId ? "open" : "closed"}>
        <Tabs
          idPrefix="ag" label={tr("agents.agenti_e_profili")} value={tab} onChange={(t) => navigate(t === "profiles" ? "/agents?tab=profiles" : "/agents")}
          tabs={[{ id: "agents", label: tr("agents.agenti"), count: agents.data?.length }, { id: "profiles", label: tr("agents.profili_di_inferenza") }]}
        />
      </div>
      <TabPanel idPrefix="ag" id={tab}>
        {tab === "profiles" ? (
          <ProfilesPanel />
        ) : (
          <QueryState
            query={agents} isEmpty={(a) => a.length === 0 && !agentId}
            empty={<EmptyState icon="agent" title={tr("agents.nessun_agente")} description={tr("agents.crea_il_primo_agente_scegli")} action={<Link to="/agents/new" className="btn primary">{tr("agents.nuovo_agente")}</Link>} />}
          >
            {(all) => (
              <MasterDetail
                className="agents-ws" detailOpen={!!agentId}
                list={<AgentList agents={all} selectedId={selectedId} />}
                detail={selectedId ? <AgentEditor agentId={selectedId} from={query.from} /> : null}
              />
            )}
          </QueryState>
        )}
      </TabPanel>
    </Page>
  );
}
