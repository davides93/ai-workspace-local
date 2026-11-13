import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import type { RemoteAgent } from "../../shared/api/types";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";

import { t } from "../../shared/i18n";
/**
 * LAN address of this device (`null` while the server only listens on this computer). Shares the
 * `mesh` query with the devices page, so opening both costs one request.
 */
export function useLanAddress() {
  const api = useApi();
  return useQuery({ queryKey: qk.mesh(), queryFn: () => api.mesh(), select: (m) => m.address });
}

export function useRemoteAgents() {
  const api = useApi();
  return useQuery({ queryKey: qk.remoteAgents(), queryFn: () => api.remoteAgents() });
}

export function useSaveRemoteAgent() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (r: RemoteAgent) => api.saveRemoteAgent(r),
    meta: { handled: true },
    onSuccess: (r) => {
      toast(t("integrations.agente_remoto_salvato", { name: r.name }));
      void qc.invalidateQueries({ queryKey: qk.remoteAgents() });
    },
  });
}

/** The failure (if any) is shown inside the confirm dialog. */
export function useDeleteRemoteAgent() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteRemoteAgent(id),
    meta: { handled: true },
    onSuccess: () => {
      toast(t("integrations.agente_remoto_eliminato"));
      void qc.invalidateQueries({ queryKey: qk.remoteAgents() });
    },
  });
}
