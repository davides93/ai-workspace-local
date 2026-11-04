import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { ignoreNotFound } from "../../shared/lib/errors";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { Session } from "../../shared/api/types";

export function useSessions() {
  const api = useApi();
  return useQuery({ queryKey: qk.sessions.list(), queryFn: () => api.sessions() });
}

export function useCreateSession() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: { agent_id: string; title?: string; isolation?: "none" | "worktree" }) => api.createSession(input),
    onSuccess: (s) => {
      qc.setQueryData<Session[]>(qk.sessions.list(), (list) => [s, ...(list ?? []).filter((x) => x.id !== s.id)]);
      void qc.invalidateQueries({ queryKey: qk.sessions.list() });
    },
  });
}

export function useRenameSession() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { id: string; title: string }) => api.renameSession(v.id, v.title),
    onSuccess: (s) => {
      // The list shows the new title at once; the refetch confirms it.
      qc.setQueryData<Session[]>(qk.sessions.list(), (list) => list?.map((x) => (x.id === s.id ? { ...x, ...s } : x)));
      void qc.invalidateQueries({ queryKey: qk.sessions.all() });
    },
  });
}

export function useDeleteSession() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    // Already deleted from another client (404): the session is gone, which is what was asked.
    mutationFn: (id: string) => ignoreNotFound(api.deleteSession(id)),
    onSuccess: (_r, id) => {
      toast("Sessione eliminata");
      // Out of the list at once, so the view never navigates back to a session that is gone.
      qc.setQueryData<Session[]>(qk.sessions.list(), (list) => list?.filter((x) => x.id !== id));
      void qc.invalidateQueries({ queryKey: qk.sessions.all() });
    },
  });
}

/** Starts a run (a user message or an A2UI action); the caller subscribes to its events. */
export function useStartRun() {
  const api = useApi();
  return useMutation({
    mutationFn: (v: { sessionId: string; input: string; kind?: "a2ui.action" }) => api.startRun(v.sessionId, v.input, v.kind),
    // The chat reports the failure itself (with the right wording): skip the generic toast.
    meta: { handled: true },
  });
}

export function useCancelRun() {
  const api = useApi();
  return useMutation({ mutationFn: (runId: string) => api.cancelRun(runId) });
}

/** Which speech-to-text backend voice mode would use here (and why not, when none is available). */
export function useVoiceStatus() {
  const api = useApi();
  return useQuery({ queryKey: qk.voice(), queryFn: () => api.voice() });
}

/** Speech to text of a recorded WAV. The voice flow reports its own failures, with the right wording. */
export function useTranscribe() {
  const api = useApi();
  return useMutation({
    mutationFn: (v: { wav: Blob; language?: string }) => api.transcribe(v.wav, v.language),
    meta: { handled: true },
  });
}

/** Changes of an isolated session; refetched by the panel whenever a run ends. */
export function useWorktree(sessionId: string, enabled: boolean) {
  const api = useApi();
  return useQuery({
    queryKey: qk.sessions.worktree(sessionId),
    queryFn: () => api.worktreeStatus(sessionId),
    enabled,
    staleTime: 0,
  });
}

export function useWorktreeApply() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (sessionId: string) => api.worktreeApply(sessionId),
    onSuccess: (st, sessionId) => {
      qc.setQueryData(qk.sessions.worktree(sessionId), st);
      toast("Modifiche applicate al workspace (non ancora committate)");
    },
  });
}

export function useWorktreeDiscard() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { sessionId: string; branch: string }) => api.worktreeDiscard(v.sessionId),
    onSuccess: (_r, v) => {
      toast(`Worktree ${v.branch} eliminato`);
      void qc.invalidateQueries({ queryKey: qk.sessions.all() });
    },
  });
}
