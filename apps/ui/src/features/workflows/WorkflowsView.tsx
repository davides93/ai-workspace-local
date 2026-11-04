import { Link, useParams, useQueryParams } from "../../app/router";
import { EmptyState, Icon, MasterDetail, Page, PageHeader, QueryState } from "../../shared/ui";
import { useSaveWorkflow, useWorkflowRuns, useWorkflows } from "./api";
import { NEW_WORKFLOW, WorkflowEditor } from "./components/WorkflowEditor";
import { WorkflowList } from "./components/WorkflowList";
import "./workflows.css";

/** `#/workflows` and `#/workflows/:workflowId` (`new` opens a blank form, `?from=` a copy). */
export function WorkflowsView() {
  const workflowId = useParams().workflowId;
  const query = useQueryParams();
  const wfs = useWorkflows();
  const runs = useWorkflowRuns();
  const toggle = useSaveWorkflow({ silent: true });
  const list = wfs.data ?? [];
  // Wide screens show the first workflow while the URL names none; on phones the list is the page.
  const selectedId = workflowId ?? list[0]?.id;

  return (
    <Page className="wf-page">
      <PageHeader
        eyebrow="Lavoro" title="Workflow"
        subtitle="Task operativi ripetibili: trigger, passi con tool o agenti, conferme umane."
        actions={<Link to={`/workflows/${NEW_WORKFLOW}`} className="btn primary"><Icon name="plus" size={15} /> Nuovo workflow</Link>}
      />
      <QueryState
        query={wfs} isEmpty={(w) => w.length === 0 && !workflowId}
        empty={<EmptyState icon="flow" title="Nessun workflow" description="Un workflow concatena passi (tool, agenti, conferme) e parte a mano, a orari fissi, quando un file cambia o da un webhook." action={<Link to={`/workflows/${NEW_WORKFLOW}`} className="btn primary">Nuovo workflow</Link>} />}
      >
        {(all) => (
          <MasterDetail
            ratio="wide" className="wf-ws" detailOpen={!!workflowId}
            list={
              <WorkflowList
                workflows={all} runs={runs.data ?? []} selectedId={selectedId}
                togglingId={toggle.isPending ? toggle.variables?.id ?? null : null}
                onToggle={(w, enabled) => toggle.mutate({ ...w, enabled })}
              />
            }
            detail={selectedId ? <WorkflowEditor workflowId={selectedId} from={query.from} /> : null}
          />
        )}
      </QueryState>
    </Page>
  );
}
