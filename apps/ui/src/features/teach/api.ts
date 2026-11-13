import { useEffect, useRef } from "react";
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { errorMessage } from "../../shared/lib/errors";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { Memory, MemoryPatch, MemoryQuery } from "../../shared/api/types";
import { useCreateSession, useSessions } from "../chat/api";
import { activated, forgotten, patched, TEACHER_ID, teachSession, without } from "./model/memories";

import { t } from "../../shared/i18n";
/** What the assistant remembers. Proposals can arrive while the page is open (the assistant saved them in a chat), so it is refreshed now and then. */
export function useMemories(query: MemoryQuery = {}) {
  const api = useApi();
  return useQuery({ queryKey: qk.memories.list(query), queryFn: () => api.memories(query), refetchInterval: 20_000, placeholderData: (prev) => prev });
}

type Snapshot = [readonly unknown[], Memory[] | undefined][];

/** Applies `change` to every cached list of memories and returns what was there, to put back on failure. */
function applyToLists(qc: QueryClient, change: (list: Memory[], key: readonly unknown[]) => Memory[]): Snapshot {
  const snapshot: Snapshot = qc.getQueriesData<Memory[]>({ queryKey: qk.memories.all() }).map(([key, data]) => [key, data]);
  for (const [key, data] of snapshot) if (data) qc.setQueryData<Memory[]>(key, change(data, key));
  return snapshot;
}

/** The status filter a cached list was asked with (its key is `["memories", "list", status, kind, q]`). */
const statusOf = (key: readonly unknown[]): string => String(key[2] ?? "");

/**
 * An action on one memory: the lists change at once (`optimistic`), a failure puts them back and says why,
 * and the truth is asked for when it is over.
 */
function useMemoryAction<V>(run: (v: V) => Promise<unknown>, optimistic: (v: V, list: Memory[], key: readonly unknown[]) => Memory[], failure: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: run,
    meta: { handled: true },
    onMutate: async (v: V) => {
      await qc.cancelQueries({ queryKey: qk.memories.all() });
      return { snapshot: applyToLists(qc, (list, key) => optimistic(v, list, key)) };
    },
    onError: (e, _v, ctx) => {
      for (const [key, data] of ctx?.snapshot ?? []) qc.setQueryData(key, data);
      toast(`${failure}: ${errorMessage(e)}`, "crit");
    },
    onSettled: () => void qc.invalidateQueries({ queryKey: qk.memories.all() }),
  });
}

export function useUpdateMemory() {
  const api = useApi();
  return useMemoryAction(
    (v: { id: string; patch: MemoryPatch }) => api.updateMemory(v.id, v.patch),
    (v, list) => patched(list, v.id, v.patch),
    "Non sono riuscito a salvare la modifica",
  );
}

/** Soft delete: the memory leaves the lists at once and can be restored. */
export function useForgetMemory() {
  const api = useApi();
  return useMemoryAction((id: string) => api.forgetMemory(id), (id, list, key) => forgotten(list, id, statusOf(key) === "all"), "Non sono riuscito a dimenticarlo");
}

export function useRestoreMemory() {
  const api = useApi();
  return useMemoryAction((m: Memory) => api.restoreMemory(m.id), (m, list, key) => (["", "active", "all"].includes(statusOf(key)) ? activated(list, m) : without(list, m.id)), "Non sono riuscito a ripristinarlo");
}

export function useApproveMemory() {
  const api = useApi();
  return useMemoryAction((m: Memory) => api.approveMemory(m.id), (m, list, key) => (statusOf(key) === "proposed" ? without(list, m.id) : activated(list, m)), "Non sono riuscito a confermarlo");
}

export function useRejectMemory() {
  const api = useApi();
  return useMemoryAction((id: string) => api.rejectMemory(id), (id, list) => without(list, id), "Non sono riuscito a scartarlo");
}

/** Deletes everything for good; the caller has asked for a confirmation. */
export function useDeleteAllMemories() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.deleteAllMemories(),
    meta: { handled: true },
    onSuccess: ({ deleted }) => {
      toast(deleted === 1 ? t("teach.ho_cancellato_1_ricordo") : t("teach.ho_cancellato_ricordi", { deleted }));
      void qc.invalidateQueries({ queryKey: qk.memories.all() });
    },
  });
}

/** Asks for the memories again (a run saved something). */
export function useRefreshMemories() {
  const qc = useQueryClient();
  return () => void qc.invalidateQueries({ queryKey: qk.memories.all() });
}

/**
 * The persistent "Insegna" conversation: the latest session of the teacher agent. There is none the
 * first time: it is created once (a ref keeps a second render from asking again), never one per visit.
 */
export function useTeachSession() {
  const sessions = useSessions();
  const create = useCreateSession();
  const found = teachSession(sessions.data);
  const asked = useRef(false);
  const ready = !!sessions.data;
  const { mutate, isPending, isError, error, reset } = create;
  useEffect(() => {
    if (!ready || found || asked.current || isPending || isError) return;
    asked.current = true;
    mutate({ agent_id: TEACHER_ID, title: t("teach.insegna"), isolation: "none" }, { onError: () => { asked.current = false; } });
  }, [ready, found, isPending, isError, mutate]);
  return {
    id: found?.id ?? null,
    loading: !found && !sessions.isError && !isError,
    error: sessions.isError ? sessions.error : isError ? error : null,
    retry: () => {
      asked.current = false;
      if (sessions.isError) void sessions.refetch();
      else reset();
    },
  };
}
