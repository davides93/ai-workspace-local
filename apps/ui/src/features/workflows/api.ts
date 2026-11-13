import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { WorkflowDef, WorkflowRun } from "../../shared/api/types";

import { t } from "../../shared/i18n";
/** Poll interval of the workflow runs, used only while one of them is still going. */
export const WORKFLOW_RUNS_POLL_MS = 2_000;

export const isActiveRun = (r: Pick<WorkflowRun, "status">): boolean => r.status === "running" || r.status === "waiting_approval";

export function useWorkflows() {
  const api = useApi();
  return useQuery({ queryKey: qk.workflows.list(), queryFn: () => api.workflows() });
}

export function useWorkflowRuns() {
  const api = useApi();
  return useQuery({
    queryKey: qk.workflows.runs(),
    queryFn: () => api.workflowRuns(),
    refetchInterval: (q) => (q.state.data?.some(isActiveRun) ? WORKFLOW_RUNS_POLL_MS : false),
  });
}

/** Agents, inference profiles and tools the steps can point at (shared keys: other pages read the same entries). */
export function useAgentChoices() {
  const api = useApi();
  return useQuery({ queryKey: qk.agents(), queryFn: () => api.agents() });
}

export function useTeamChoices() {
  const api = useApi();
  return useQuery({ queryKey: qk.teams(), queryFn: () => api.teams() });
}

export function useProfileChoices() {
  const api = useApi();
  return useQuery({ queryKey: qk.profiles(), queryFn: () => api.profiles() });
}

export function useToolChoices() {
  const api = useApi();
  return useQuery({ queryKey: qk.tools(), queryFn: () => api.tools() });
}

/** Puts the saved workflow in the cached list at once, so the screen does not wait for the refetch. */
function upsert(list: WorkflowDef[] | undefined, w: WorkflowDef): WorkflowDef[] {
  const all = list ?? [];
  return all.some((x) => x.id === w.id) ? all.map((x) => (x.id === w.id ? w : x)) : [...all, w];
}

export function useSaveWorkflow(opts: { silent?: boolean } = {}) {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (w: WorkflowDef) => api.saveWorkflow(w),
    onSuccess: (w) => {
      if (!opts.silent) toast(t("workflows.workflow_salvato"));
      qc.setQueryData<WorkflowDef[]>(qk.workflows.list(), (list) => upsert(list, w));
      void qc.invalidateQueries({ queryKey: qk.workflows.list() });
    },
  });
}

/** Failures are shown inside the confirm dialog. */
export function useDeleteWorkflow() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteWorkflow(id),
    meta: { handled: true },
    onSuccess: (_void, id) => {
      toast(t("workflows.workflow_eliminato"));
      qc.setQueryData<WorkflowDef[]>(qk.workflows.list(), (list) => list?.filter((w) => w.id !== id));
      void qc.invalidateQueries({ queryKey: qk.workflows.all() });
    },
  });
}

/** Starts a run of the saved workflow; the run is in the cached list right away so its detail can open. */
export function useRunWorkflow() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (w: Pick<WorkflowDef, "id" | "inputs">) => api.runWorkflow(w.id, w.inputs),
    onSuccess: (run) => {
      toast(t("workflows.workflow_avviato"));
      qc.setQueryData<WorkflowRun[]>(qk.workflows.runs(), (list) => [run, ...(list ?? []).filter((r) => r.id !== run.id)]);
      void qc.invalidateQueries({ queryKey: qk.workflows.runs() });
    },
  });
}
