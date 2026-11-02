import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { WorkflowDef, WorkflowRun } from "../../shared/api/types";

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

export function useSaveWorkflow(opts: { silent?: boolean } = {}) {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (w: WorkflowDef) => api.saveWorkflow(w),
    onSuccess: () => {
      if (!opts.silent) toast("Workflow salvato");
      void qc.invalidateQueries({ queryKey: qk.workflows.list() });
    },
  });
}

export function useDeleteWorkflow() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteWorkflow(id),
    onSuccess: () => {
      toast("Workflow eliminato");
      void qc.invalidateQueries({ queryKey: qk.workflows.all() });
    },
  });
}

export function useRunWorkflow() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (w: WorkflowDef) => api.runWorkflow(w.id, w.inputs),
    onSuccess: (_run, w) => {
      toast(`Workflow "${w.name}" avviato`);
      void qc.invalidateQueries({ queryKey: qk.workflows.runs() });
    },
  });
}
