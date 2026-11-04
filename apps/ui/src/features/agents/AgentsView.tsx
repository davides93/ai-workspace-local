import { Link, navigate, useParams, useQueryParams } from "../../app/router";
import { EmptyState, Icon, MasterDetail, Page, PageHeader, QueryState, TabPanel, Tabs } from "../../shared/ui";
import { useAgents } from "./api";
import { AgentEditor } from "./components/AgentEditor";
import { AgentList } from "./components/AgentList";
import { ProfilesPanel } from "./components/ProfilesPanel";
import "./agents.css";

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
        eyebrow="Lavoro" title="Agenti"
        subtitle="Un agente unisce un profilo di inferenza, un set di tool e una politica di approvazione."
        actions={tab === "agents" && <Link to="/agents/new" className="btn primary"><Icon name="plus" size={15} /> Nuovo agente</Link>}
      />
      <div className="agents-tabs" data-detail={agentId ? "open" : "closed"}>
        <Tabs
          idPrefix="ag" label="Agenti e profili" value={tab} onChange={(t) => navigate(t === "profiles" ? "/agents?tab=profiles" : "/agents")}
          tabs={[{ id: "agents", label: "Agenti", count: agents.data?.length }, { id: "profiles", label: "Profili di inferenza" }]}
        />
      </div>
      <TabPanel idPrefix="ag" id={tab}>
        {tab === "profiles" ? (
          <ProfilesPanel />
        ) : (
          <QueryState
            query={agents} isEmpty={(a) => a.length === 0 && !agentId}
            empty={<EmptyState icon="agent" title="Nessun agente" description="Crea il primo agente: scegli un profilo di inferenza, i tool e la modalità di approvazione." action={<Link to="/agents/new" className="btn primary">Nuovo agente</Link>} />}
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
