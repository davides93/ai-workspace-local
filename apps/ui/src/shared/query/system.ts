import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../api/context";
import { isUnauthorized } from "../lib/errors";
import { toast } from "../lib/toast";
import { qk } from "./keys";

/** Polling interval of the system snapshot (device, version, pairing code, peers). */
export const SYSTEM_POLL_MS = 15_000;

export function useSystem() {
  const api = useApi();
  return useQuery({ queryKey: qk.system(), queryFn: () => api.system(), refetchInterval: SYSTEM_POLL_MS });
}

/**
 * Connection state derived from the system query: a 401 means "pair this device first", any
 * other failure means the workspace is unreachable (the shell shows the connection banner).
 */
export function useConnection() {
  const q = useSystem();
  const unauthorized = q.isError && isUnauthorized(q.error);
  return {
    system: q.data,
    needsPairing: unauthorized,
    lost: q.isError && !unauthorized,
    retry: () => void q.refetch(),
    retrying: q.isFetching,
  };
}

/** Emergency stop: runs, sidecars started by runs and the controlled browser. */
export function useKill() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.kill(),
    onSuccess: () => {
      toast("Kill switch attivato per run, addestramenti, runtime gestiti e browser");
      void qc.invalidateQueries({ queryKey: qk.runtimes.all() });
      void qc.invalidateQueries({ queryKey: qk.sessions.all() });
      void qc.invalidateQueries({ queryKey: qk.approvals() });
      void qc.invalidateQueries({ queryKey: qk.training.all() });
    },
    onError: (e) => toast(`Kill switch non riuscito: ${String(e instanceof Error ? e.message : e)}`, "crit"),
  });
}
