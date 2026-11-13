import { Link, useParams, useQueryParams } from "../../app/router";
import { EmptyState, Icon, MasterDetail, Page, PageHeader, QueryState } from "../../shared/ui";
import { useSaveWorkflow, useWorkflowRuns, useWorkflows } from "./api";
import { NEW_WORKFLOW, WorkflowEditor } from "./components/WorkflowEditor";
import { WorkflowList } from "./components/WorkflowList";
import "./workflows.css";

import { t } from "../../shared/i18n";
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
        eyebrow={t("nav.group_work")} title={t("workflows.workflow")}
        subtitle={t("workflows.task_operativi_ripetibili_trigger_passi")}
        actions={<Link to={`/workflows/${NEW_WORKFLOW}`} className="btn primary"><Icon name="plus" size={15} /> {t("workflows.nuovo_workflow")}</Link>}
      />
      <QueryState
        query={wfs} isEmpty={(w) => w.length === 0 && !workflowId}
        empty={<EmptyState icon="flow" title={t("workflows.nessun_workflow")} description={t("workflows.un_workflow_concatena_passi_tool")} action={<Link to={`/workflows/${NEW_WORKFLOW}`} className="btn primary">{t("workflows.nuovo_workflow")}</Link>} />}
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
