import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { RuntimeInfo } from "../../shared/api/types";

/** Polling of the runtime list while the runtimes view is open. */
export const RUNTIMES_POLL_MS = 5_000;
/** Polling of the process log while a runtime drawer is open. */
export const LOGS_POLL_MS = 2_000;

export function useRuntimes(opts: { poll?: boolean } = {}) {
  const api = useApi();
  return useQuery({
    queryKey: qk.runtimes.list(),
    queryFn: () => api.runtimes(),
    refetchInterval: opts.poll ? RUNTIMES_POLL_MS : false,
  });
}

/** Re-runs the detection of every runtime (POST /api/runtimes/refresh). */
export function useRefreshRuntimes() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.refreshRuntimes(),
    onSuccess: (list) => {
      qc.setQueryData<RuntimeInfo[]>(qk.runtimes.list(), list);
      void qc.invalidateQueries({ queryKey: qk.models.installed() });
      toast("Rilevamento dei runtime completato");
    },
  });
}

export function useRuntimeAction() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { runtime: RuntimeInfo; action: "start" | "stop"; model?: string }) =>
      api.runtimeAction(v.runtime.id, v.action, v.model ? { model: v.model } : undefined),
    onSuccess: (_r, v) => {
      toast(v.model ? `${v.runtime.name} avviato con ${v.model}` : `${v.runtime.name}: ${v.action === "start" ? "avviato" : "fermato"}`);
      void qc.invalidateQueries({ queryKey: qk.runtimes.all() });
    },
  });
}

export function useUpdateRuntimeSettings() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { id: string; settings: Record<string, unknown> }) => api.updateRuntimeSettings(v.id, v.settings),
    onSuccess: (r) => {
      toast(`Impostazioni di ${r.name} salvate`);
      void qc.invalidateQueries({ queryKey: qk.runtimes.all() });
    },
  });
}

/** Process log of a runtime; `poll` refreshes it every couple of seconds (the drawer is open and live). */
export function useRuntimeLogs(id: string, opts: { poll: boolean }) {
  const api = useApi();
  return useQuery({
    queryKey: qk.runtimes.logs(id),
    queryFn: () => api.runtimeLogs(id),
    refetchInterval: opts.poll ? LOGS_POLL_MS : false,
  });
}
