import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { EvalReport, EvalRunRequest, EvalSuite, OptimizeRequest, PromptOptimization } from "../../shared/api/types";
import { EVAL_POLL_MS, isRunning } from "./model/evals";
import { isOptimizing, OPT_POLL_MS } from "./model/optimizations";

import { t } from "../../shared/i18n";
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
      toast(t("evals.suite_salvata"));
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
      toast(t("evals.suite_eliminata"));
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
      toast(t("evals.valutazione_avviata"));
      qc.setQueryData<EvalReport[]>(qk.evals.reports(), (list) => [report, ...(list ?? []).filter((r) => r.id !== report.id)]);
      void qc.invalidateQueries({ queryKey: qk.evals.reports() });
    },
  });
}

/** Optimizations of the prompt, most recent first; refreshed while one of them is still going. */
export function useOptimizations() {
  const api = useApi();
  return useQuery({
    queryKey: qk.evals.optimizations(),
    queryFn: () => api.promptOptimizations(),
    refetchInterval: (q) => (q.state.data?.some(isOptimizing) ? OPT_POLL_MS : false),
  });
}

/** Starts the optimization in the background; it is in the cached list at once so the page can open it. */
export function useStartOptimization() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { suiteId: string } & OptimizeRequest) => {
      const { suiteId, ...req } = v;
      return api.startPromptOptimization(suiteId, req);
    },
    meta: { handled: true },
    onSuccess: (o) => {
      toast(t("evals.ottimizzazione_avviata"));
      qc.setQueryData<PromptOptimization[]>(qk.evals.optimizations(), (list) => [o, ...(list ?? []).filter((x) => x.id !== o.id)]);
      void qc.invalidateQueries({ queryKey: qk.evals.optimizations() });
    },
  });
}

/** Saves the prompt of a variant in the agent. The agent list is stale after this, so it is refreshed. */
export function useApplyOptimization() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { id: string; variant: number }) => api.applyPromptOptimization(v.id, v.variant),
    meta: { handled: true },
    onSuccess: (o) => {
      toast(t("evals.prompt_applicato_all_agente"));
      qc.setQueryData<PromptOptimization[]>(qk.evals.optimizations(), (list) => list?.map((x) => (x.id === o.id ? o : x)));
      void qc.invalidateQueries({ queryKey: qk.evals.optimizations() });
      void qc.invalidateQueries({ queryKey: qk.agents() });
    },
  });
}
