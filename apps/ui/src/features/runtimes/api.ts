import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { InstallJob, RuntimeInfo } from "../../shared/api/types";

import { t } from "../../shared/i18n";
/** Polling of the runtime list while the runtimes view is open. */
export const RUNTIMES_POLL_MS = 5_000;
/** Polling of a running installation. */
export const INSTALL_POLL_MS = 1_000;
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
      toast(t("runtimes.rilevamento_dei_runtime_completato"));
    },
  });
}

export function useRuntimeAction(handled = false) {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    meta: { handled },
    mutationFn: (v: { runtime: RuntimeInfo; action: "start" | "stop"; model?: string }) =>
      api.runtimeAction(v.runtime.id, v.action, v.model ? { model: v.model } : undefined),
    onSuccess: (_r, v) => {
      toast(v.model ? t("runtimes.avviato_con", { name: v.runtime.name, model: v.model }) : t(v.action === "start" ? "runtimes.started_name" : "runtimes.stopped_name", { name: v.runtime.name }));
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
      toast(t("runtimes.impostazioni_di_salvate", { name: r.name }));
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

/** Steps and missing prerequisites of installing a runtime on this device. */
export function useInstallPlan(id: string, enabled = true) {
  const api = useApi();
  return useQuery({ queryKey: qk.runtimes.installPlan(id), queryFn: () => api.runtimeInstallPlan(id), enabled });
}

/** The last installation of a runtime; polled every second while it runs, and the runtime list is
 *  refreshed once it ends (the server detects the runtimes again by itself). */
export function useInstallJob(id: string) {
  const api = useApi();
  const qc = useQueryClient();
  return useQuery({
    queryKey: qk.runtimes.install(id),
    queryFn: async (): Promise<InstallJob | null> => {
      const job = await api.runtimeInstall(id);
      if (job && job.state !== "running") {
        void qc.invalidateQueries({ queryKey: qk.runtimes.list() });
        void qc.invalidateQueries({ queryKey: qk.models.installed() });
      }
      return job;
    },
    refetchInterval: (q) => (q.state.data?.state === "running" ? INSTALL_POLL_MS : false),
  });
}

/** Installs a runtime by itself, no further confirmation: one click. */
export function useInstallRuntime() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (runtime: Pick<RuntimeInfo, "id" | "name">) => api.installRuntime(runtime.id, runtime.id === "localai"),
    meta: { handled: true },
    onSuccess: (job, runtime) => {
      qc.setQueryData(qk.runtimes.install(runtime.id), job);
      toast(t("runtimes.installazione_di_avviata", { name: runtime.name }));
    },
  });
}
