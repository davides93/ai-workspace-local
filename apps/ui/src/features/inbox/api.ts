import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { errorMessage } from "../../shared/lib/errors";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { ProposalQuery } from "../../shared/api/types";

import { t } from "../../shared/i18n";
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
