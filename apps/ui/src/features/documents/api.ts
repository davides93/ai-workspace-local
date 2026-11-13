import { useEffect, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { blobToBase64 } from "../../shared/lib/base64";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { KbCollectionCreate, KbCollectionPatch, KbJob, KbSearchBody } from "../../shared/api/types";
import { runningJobs, splitBatches } from "./model/kb";

import { t } from "../../shared/i18n";
export function useKbCollections() {
  const api = useApi();
  return useQuery({ queryKey: qk.kb.collections(), queryFn: () => api.kbCollections() });
}

export function useKbDocuments(collectionId: string | null) {
  const api = useApi();
  return useQuery({ queryKey: qk.kb.documents(collectionId ?? ""), queryFn: () => api.kbDocuments(collectionId!), enabled: collectionId !== null });
}

/** Jobs and the embedding model; asked every second while something is running, now and then otherwise. */
export function useKbStatus() {
  const api = useApi();
  return useQuery({
    queryKey: qk.kb.status(),
    queryFn: () => api.kbStatus(),
    refetchInterval: (q) => (runningJobs(q.state.data).length > 0 ? 1000 : 20_000),
  });
}

/** When a job ends, the lists (counts, documents) are asked again. */
export function useKbSync(running: number) {
  const qc = useQueryClient();
  const before = useRef(running);
  useEffect(() => {
    if (before.current > running) void qc.invalidateQueries({ queryKey: qk.kb.all() });
    before.current = running;
  }, [running, qc]);
}

const refresh = (qc: ReturnType<typeof useQueryClient>) => qc.invalidateQueries({ queryKey: qk.kb.all() });

export function useCreateCollection() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (b: KbCollectionCreate) => api.createKbCollection(b), onSuccess: () => void refresh(qc) });
}

export function useUpdateCollection() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (v: { id: string; patch: KbCollectionPatch }) => api.updateKbCollection(v.id, v.patch), onSuccess: () => void refresh(qc) });
}

export function useDeleteCollection() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteKbCollection(id),
    onSuccess: () => {
      toast(t("documents.raccolta_eliminata"));
      void refresh(qc);
      void qc.invalidateQueries({ queryKey: qk.agents() });
    },
  });
}

export function useDeleteDocument() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (id: string) => api.deleteKbDocument(id), onSuccess: () => void refresh(qc) });
}

/** Sends the files in groups the server accepts; the work goes on in the background (see `useKbStatus`). */
export function useAddFiles() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (v: { collectionId: string; files: File[] }) => {
      const { batches, errors } = splitBatches(v.files);
      const jobs: KbJob[] = [];
      for (const batch of batches) {
        const files = await Promise.all(batch.map(async (f) => ({ name: f.name, mime: f.type || "application/octet-stream", data: await blobToBase64(f) })));
        jobs.push((await api.addKbFiles(v.collectionId, files)).job);
      }
      return { jobs, errors };
    },
    onSettled: () => void qc.invalidateQueries({ queryKey: qk.kb.status() }),
  });
}

export function useAddPath() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (v: { collectionId: string; path: string }) => api.addKbPath(v.collectionId, v.path), onSettled: () => void qc.invalidateQueries({ queryKey: qk.kb.status() }) });
}

export function useReindex() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (collectionId: string) => api.reindexKbCollection(collectionId), onSettled: () => void qc.invalidateQueries({ queryKey: qk.kb.status() }) });
}

export function useCancelJob() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (jobId: string) => api.cancelKbJob(jobId), onSettled: () => void qc.invalidateQueries({ queryKey: qk.kb.status() }) });
}

export function useKbSearch() {
  const api = useApi();
  return useMutation({ mutationFn: (b: KbSearchBody) => api.kbSearch(b) });
}
