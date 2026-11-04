import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { ApprovalRequest } from "../../shared/api/types";

const APPROVALS_POLL_MS = 3_000;

/** Pending confirmations of agents, sub-agents and unattended workflows. */
export function useApprovals() {
  const api = useApi();
  return useQuery({ queryKey: qk.approvals(), queryFn: () => api.approvals(), refetchInterval: APPROVALS_POLL_MS });
}

export function useRespondApproval(opts: { silent?: boolean } = {}) {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { request: ApprovalRequest; decision: "allow" | "deny"; remember?: boolean }) =>
      api.respondApproval(v.request.id, v.decision, v.remember),
    onSuccess: (_r, v) => {
      if (!opts.silent) toast(v.decision === "allow" ? "Consentito" : "Negato");
      qc.setQueryData<ApprovalRequest[]>(qk.approvals(), (list) => list?.filter((a) => a.id !== v.request.id));
    },
    // Also after a failure (the request may have been handled elsewhere): the list is the truth.
    onSettled: () => void qc.invalidateQueries({ queryKey: qk.approvals() }),
  });
}
