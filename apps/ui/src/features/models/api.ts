import { useEffect, useRef } from "react";
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { DownloadProgress, ModelSearchParams, PullRequest } from "../../shared/api/types";
import { isActiveJob, type RetryPlan } from "./model/downloads";
import { shouldRetryHub } from "./model/errors";

/** Downloads are polled once a second, but only while a job is waiting, transferring or verifying. */
export const DOWNLOADS_POLL_MS = 1_000;

export const isActiveDownload = (d: Pick<DownloadProgress, "status">): boolean => isActiveJob(d);

/** `refetchInterval` rule of the downloads query: poll only while there is an active job. */
export const downloadsInterval = (data: DownloadProgress[] | undefined): number | false =>
  data?.some(isActiveDownload) ? DOWNLOADS_POLL_MS : false;

export function useModels() {
  const api = useApi();
  return useQuery({ queryKey: qk.models.installed(), queryFn: () => api.models() });
}

export function useCatalog() {
  const api = useApi();
  return useQuery({ queryKey: qk.models.catalog(), queryFn: () => api.catalog() });
}

/**
 * The model adviser. An older server or another transport may not have the endpoint: the query then
 * fails (once, no retries) and callers fall back to what they computed before.
 */
export function useRecommend() {
  const api = useApi();
  return useQuery({ queryKey: qk.models.recommend(), queryFn: () => api.recommend(), retry: false, staleTime: 60_000 });
}

/** The runtimes that can run `model` here, best first (the runtime router). Off while there is no model. */
export function useRoute(model: string, context: number | undefined, remote: boolean) {
  const api = useApi();
  return useQuery({
    queryKey: qk.models.route(model, context, remote),
    queryFn: () => api.route(model, context, remote),
    enabled: model.trim() !== "",
    retry: false,
    staleTime: 15_000,
  });
}

export function useDownloads() {
  const api = useApi();
  return useQuery({
    queryKey: qk.models.downloads(),
    queryFn: () => api.downloads(),
    refetchInterval: (q) => downloadsInterval(q.state.data),
  });
}

/**
 * Mounted once by the shell: when a job reaches `done` the installed models, the catalog and the
 * runtime compatibility are refreshed (and the user is told), whichever view is open.
 */
export function useDownloadsWatcher() {
  const qc = useQueryClient();
  const { data } = useDownloads();
  const seen = useRef<Map<string, DownloadProgress["status"]> | null>(null);
  useEffect(() => {
    if (!data) return;
    const prev = seen.current;
    seen.current = new Map(data.map((d) => [d.id, d.status]));
    if (!prev) return;
    for (const d of data) {
      const before = prev.get(d.id);
      if (d.status === "done" && before !== undefined && before !== "done") {
        toast(`Download completato: ${d.model_ref || d.model_id}`);
        void qc.invalidateQueries({ queryKey: qk.models.installed() });
        void qc.invalidateQueries({ queryKey: qk.models.catalog() });
        void qc.invalidateQueries({ queryKey: qk.models.recommend() });
        void qc.invalidateQueries({ queryKey: qk.models.remoteAll() });
      }
      if (d.status === "error" && before !== undefined && before !== "error") toast(`Download non riuscito: ${d.error ?? d.model_ref}`, "crit");
    }
  }, [data, qc]);
}

/**
 * Starts a download. By default a failure becomes the central error toast; with `handled` the caller
 * shows `mutation.error` itself (inline, with the "Imposta token" shortcut for 401/403).
 */
export function usePullModel(opts: { handled?: boolean } = {}) {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (req: string | PullRequest) => api.pullModel(req),
    onSuccess: () => void qc.invalidateQueries({ queryKey: qk.models.downloads() }),
    meta: opts.handled ? { handled: true } : undefined,
  });
}

/**
 * "Riprova" of a failed or cancelled job whose request can be rebuilt: pulls it again and, once the
 * new job exists, drops the old one so the list does not show the same download twice.
 */
export function useRetryDownload() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (v: { job: DownloadProgress; plan: Extract<RetryPlan, { kind: "pull" }> }) => {
      const created = await api.pullModel(v.plan.request);
      if (created.id !== v.job.id) await api.clearDownload(v.job.id).catch(() => undefined);
      return created;
    },
    onSuccess: () => {
      toast("Download riavviato");
      void qc.invalidateQueries({ queryKey: qk.models.downloads() });
    },
  });
}

/** Removes every finished (done or cancelled) job from the list in one go. */
export function useClearFinishedDownloads() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (ids: string[]) => {
      await Promise.all(ids.map((id) => api.clearDownload(id)));
      return ids.length;
    },
    onSuccess: (n) => {
      toast(n === 1 ? "Download rimosso dalla lista" : `${n} download rimossi dalla lista`);
    },
    onSettled: () => void qc.invalidateQueries({ queryKey: qk.models.downloads() }),
  });
}

export function useCancelDownload() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.cancelDownload(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: qk.models.downloads() }),
  });
}

export function useClearDownload() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.clearDownload(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: qk.models.downloads() }),
  });
}

export function useDeleteModel() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteModel(id),
    onSuccess: () => {
      toast("Modello eliminato dal disco");
      void qc.invalidateQueries({ queryKey: qk.models.all() });
    },
  });
}

export function useModelProviders() {
  const api = useApi();
  return useQuery({ queryKey: qk.models.providers(), queryFn: () => api.modelProviders(), staleTime: 60_000 });
}

/**
 * `token` undefined leaves the stored token alone, `null` removes it; `endpoint` undefined leaves it,
 * `""` restores the default. The dialog shows the error (e.g. a token the provider refuses) itself.
 */
export function useSetProviderAuth() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { id: string; token?: string | null; endpoint?: string }) => api.setProviderAuth(v.id, v.token, v.endpoint),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: qk.models.providers() });
      // A gated repository that failed without a token can work now.
      void qc.invalidateQueries({ queryKey: qk.models.remoteAll() });
    },
    meta: { handled: true },
  });
}

/** Paged live search on a provider; `enabled` is false until there is something to search. */
export function useModelSearch(params: Omit<ModelSearchParams, "cursor">, enabled = true) {
  const api = useApi();
  return useInfiniteQuery({
    queryKey: qk.models.search(params),
    queryFn: ({ pageParam }) => api.searchModels({ ...params, cursor: pageParam }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next_cursor,
    enabled,
    staleTime: 60_000,
    retry: shouldRetryHub,
  });
}

/** Files, variants and runtime compatibility of one remote model. */
export function useRemoteModel(provider: string, model: string | null, revision?: string) {
  const api = useApi();
  return useQuery({
    queryKey: qk.models.remote(provider, model ?? "", revision),
    queryFn: () => api.remoteModel(provider, model as string, revision),
    enabled: !!model,
    staleTime: 60_000,
    retry: shouldRetryHub,
  });
}
