import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { errorMessage, errorStatus, isNotFound } from "../../shared/lib/errors";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { ApprovalRequest, BackgroundTask, BudgetLimits, ProposalQuery } from "../../shared/api/types";
import { useRespondApproval } from "../approvals/api";
import { pollInterval } from "./model/tasks";

import { t, type MessageKey } from "../../shared/i18n";
const POLL_MS = 20_000;

/** What the assistant noticed. The heartbeat adds items while the page is open, so it is refreshed now and then. */
export function useProposals(query: ProposalQuery = {}) {
  const api = useApi();
  return useQuery({ queryKey: qk.proposals.list(query), queryFn: () => api.proposals(query), refetchInterval: POLL_MS, placeholderData: (prev) => prev });
}

export function useProactiveStatus() {
  const api = useApi();
  return useQuery({ queryKey: qk.proactive(), queryFn: () => api.proactiveStatus(), refetchInterval: POLL_MS });
}

function useRefresh() {
  const qc = useQueryClient();
  return () => {
    void qc.invalidateQueries({ queryKey: qk.proposals.all() });
    void qc.invalidateQueries({ queryKey: qk.proactive() });
    void qc.invalidateQueries({ queryKey: qk.sessions.all() });
    // Approving a proposal starts a background task.
    void qc.invalidateQueries({ queryKey: qk.tasks.all() });
  };
}

/** Approving starts a normal run in a new session; the card moves to "Fatte" and says where the work went. */
export function useApproveProposal() {
  const api = useApi();
  const refresh = useRefresh();
  return useMutation({
    mutationFn: (v: { id: string; instruction?: string }) => api.approveProposal(v.id, v.instruction),
    meta: { handled: true },
    onSuccess: () => toast(t("inbox.ho_cominciato_trovi_il_lavoro")),
    onError: (e) => toast(t("inbox.non_sono_riuscito_ad_avviarla", { error: errorMessage(e) }), "crit"),
    onSettled: refresh,
  });
}

export function useDismissProposal() {
  const api = useApi();
  const refresh = useRefresh();
  return useMutation({
    mutationFn: (id: string) => api.dismissProposal(id),
    meta: { handled: true },
    onError: (e) => toast(t("inbox.non_sono_riuscito_a_scartarla", { error: errorMessage(e) }), "crit"),
    onSettled: refresh,
  });
}

/** "Controlla adesso": wakes the assistant once. */
export function useProactiveTick() {
  const api = useApi();
  const refresh = useRefresh();
  return useMutation({
    mutationFn: () => api.proactiveTick(),
    meta: { handled: true },
    onSuccess: (r) => toast(r.outcome === "proposed" ? t("inbox.ho_trovato_qualcosa_da_proporti") : r.reason ?? t("inbox.niente_di_nuovo")),
    onError: (e) => toast(t("inbox.il_controllo_non_e_riuscito", { error: errorMessage(e) }), "crit"),
    onSettled: refresh,
  });
}

/** How many proposals wait for the person (the badge). */
export function useOpenProposalCount(): number {
  const q = useProposals({ status: "open", kind: "proposal" });
  return q.data?.length ?? 0;
}

// ---- background tasks: what the assistant does while the person is away

/**
 * Refreshed every 4 s while something is at work (or stopping), else every 20 s. An engine that has no tasks (404) is not asked again.
 */
export function useTasks() {
  const api = useApi();
  return useQuery({
    queryKey: qk.tasks.list(), queryFn: () => api.tasks(), placeholderData: (prev) => prev,
    refetchInterval: (q) => (isNotFound(q.state.error) ? false : pollInterval(q.state.data)),
  });
}

/** Puts the engine's answer in the list at once, then asks again. */
function useTaskRefresh() {
  const qc = useQueryClient();
  return (task?: BackgroundTask) => {
    if (task) qc.setQueryData<BackgroundTask[]>(qk.tasks.list(), (list) => list?.map((x) => (x.id === task.id ? task : x)));
    void qc.invalidateQueries({ queryKey: qk.tasks.all() });
    void qc.invalidateQueries({ queryKey: qk.approvals() });
  };
}

/** A refusal for a rule (409) already says in a sentence what to do: it is shown as the engine wrote it. Any other failure gets its lead-in. */
const failure = (lead: MessageKey, e: unknown): string => (errorStatus(e) === 409 ? errorMessage(e) : t(lead, { error: errorMessage(e) }));

/** Stops between two steps: a task at work answers `stopping` and pauses when the step ends. */
export function usePauseTask() {
  const api = useApi();
  const refresh = useTaskRefresh();
  return useMutation({
    mutationFn: (id: string) => api.pauseTask(id),
    meta: { handled: true },
    onSuccess: (task) => toast(task.stopping ? t("inbox.task_toast_stopping") : t("inbox.task_toast_paused")),
    onError: (e) => toast(failure("inbox.task_err_pause", e), "crit"),
    onSettled: (task) => refresh(task),
  });
}

export interface ResumeRequest {
  id: string;
  /** Goes past quiet hours, battery or the day's time. */
  force?: boolean;
  /** Adds to the task's own budget. */
  extend?: BudgetLimits;
}

/** A 409 comes back as the engine's sentence (it points to «Riprendi comunque» or to adding time). */
export function useResumeTask() {
  const api = useApi();
  const refresh = useTaskRefresh();
  return useMutation({
    mutationFn: (v: ResumeRequest) => api.resumeTask(v.id, { ...(v.force ? { force: true } : {}), ...(v.extend ? { extend: v.extend } : {}) }),
    meta: { handled: true },
    onSuccess: (_task, v) => toast(v.force ? t("inbox.task_toast_resumed_anyway") : v.extend ? t("inbox.task_toast_time_added") : t("inbox.task_toast_resumed")),
    onError: (e) => toast(failure("inbox.task_err_resume", e), "crit"),
    onSettled: (task) => refresh(task),
  });
}

/** Says something to a task. A running task reads it at the next step; a paused one keeps it for when it resumes. */
export function useSendTaskInstruction() {
  const api = useApi();
  const refresh = useTaskRefresh();
  return useMutation({
    mutationFn: (v: { id: string; text: string; source: "user" | "voice" }) => api.sendTaskInstruction(v.id, v.text, v.source),
    meta: { handled: true },
    // A task that is finishing its step (paused, or stopped by a rule) does not read it before it resumes.
    onSuccess: (task) => toast(task.status === "running" && !task.stopping ? t("inbox.task_toast_instruction_sent") : t("inbox.task_toast_instruction_kept")),
    onError: (e) => toast(failure("inbox.task_err_instruction", e), "crit"),
    onSettled: (task) => refresh(task),
  });
}

export function useCancelTask() {
  const api = useApi();
  const refresh = useTaskRefresh();
  return useMutation({
    mutationFn: (id: string) => api.cancelTask(id),
    meta: { handled: true },
    onSuccess: () => toast(t("inbox.task_toast_cancelled")),
    onError: (e) => toast(failure("inbox.task_err_cancel", e), "crit"),
    onSettled: (task) => refresh(task),
  });
}

/**
 * Consenti / Nega on the confirmation a task waits for: the same request and the same route as the ones of the chat
 * (`respondApproval`), then the list of tasks is read again so that the task goes on in front of the person.
 */
export function useRespondTaskApproval() {
  const respond = useRespondApproval();
  const refresh = useTaskRefresh();
  return {
    pending: respond.isPending ? respond.variables?.request.id : undefined,
    respond: (request: ApprovalRequest, decision: "allow" | "deny") => respond.mutate({ request, decision }, { onSettled: () => refresh() }),
  };
}
