import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { EvalReport, EvalRunRequest, EvalSuite } from "../../shared/api/types";
import { EVAL_POLL_MS, isRunning } from "./model/evals";

export function useEvalSuites() {
  const api = useApi();
  return useQuery({ queryKey: qk.evals.suites(), queryFn: () => api.evalSuites() });
}

/** Reports, most recent first; refreshed while one of them is still running. */
export function useEvalReports() {
  const api = useApi();
  return useQuery({
    queryKey: qk.evals.reports(),
    queryFn: () => api.evalReports(),
    refetchInterval: (q) => (q.state.data?.some(isRunning) ? EVAL_POLL_MS : false),
  });
}

export function useEvalReport(id: string | undefined) {
  const api = useApi();
  return useQuery({
    queryKey: qk.evals.report(id ?? ""),
    queryFn: () => api.evalReport(id!),
    enabled: !!id,
    refetchInterval: (q) => (q.state.data && isRunning(q.state.data) ? EVAL_POLL_MS : false),
  });
}

export function useEvalComparison(a: string | undefined, b: string | undefined) {
  const api = useApi();
  return useQuery({
    queryKey: qk.evals.compare(a ?? "", b ?? ""),
    queryFn: () => api.compareEvalReports(a!, b!),
    enabled: !!a && !!b,
  });
}

/** The reason the server refuses a suite is shown next to the editor, not as a toast. */
export function useSaveEvalSuite() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: EvalSuite) => api.saveEvalSuite(s),
    meta: { handled: true },
    onSuccess: (s) => {
      toast("Suite salvata");
      qc.setQueryData<EvalSuite[]>(qk.evals.suites(), (list) => {
        const all = list ?? [];
        return all.some((x) => x.id === s.id) ? all.map((x) => (x.id === s.id ? s : x)) : [...all, s];
      });
      void qc.invalidateQueries({ queryKey: qk.evals.suites() });
    },
  });
}

export function useDeleteEvalSuite() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteEvalSuite(id),
    meta: { handled: true },
    onSuccess: (_void, id) => {
      toast("Suite eliminata");
      qc.setQueryData<EvalSuite[]>(qk.evals.suites(), (list) => list?.filter((s) => s.id !== id));
      void qc.invalidateQueries({ queryKey: qk.evals.suites() });
    },
  });
}

/** Starts a run; the report is in the cached list at once so the page can open it. */
export function useStartEval() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { suiteId: string } & EvalRunRequest) => {
      const { suiteId, ...req } = v;
      return api.startEval(suiteId, req);
    },
    meta: { handled: true },
    onSuccess: (report) => {
      toast("Valutazione avviata");
      qc.setQueryData<EvalReport[]>(qk.evals.reports(), (list) => [report, ...(list ?? []).filter((r) => r.id !== report.id)]);
      void qc.invalidateQueries({ queryKey: qk.evals.reports() });
    },
  });
}
