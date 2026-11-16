import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { qk } from "../../shared/query/keys";
import type { ChannelCreate, ChannelPatch, ContextCreate, EmailDraftCreate, EmailDraftPatch, EmailDraftSend, MailAccount } from "../../shared/api/types";

/** The channels with their live status, asked again every few seconds while the page is open. */
export function useChannels() {
  const api = useApi();
  return useQuery({ queryKey: qk.channels.list(), queryFn: () => api.channels(), refetchInterval: 5000 });
}

export function useCreateChannel() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (b: ChannelCreate) => api.createChannel(b), onSuccess: () => void qc.invalidateQueries({ queryKey: qk.channels.all() }) });
}

export function useUpdateChannel() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (v: { id: string; patch: ChannelPatch }) => api.updateChannel(v.id, v.patch), onSuccess: () => void qc.invalidateQueries({ queryKey: qk.channels.all() }) });
}

export function useDeleteChannel() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (id: string) => api.deleteChannel(id), onSuccess: () => void qc.invalidateQueries({ queryKey: qk.channels.all() }) });
}

export function useTestChannel() {
  const api = useApi();
  return useMutation({ mutationFn: (id: string) => api.testChannel(id) });
}

export function usePairChannel() {
  const api = useApi();
  return useMutation({ mutationFn: (id: string) => api.pairChannel(id) });
}

export function useMailAccounts() {
  const api = useApi();
  return useQuery({ queryKey: qk.mailAccounts(), queryFn: () => api.mailAccounts() });
}

export function useSetMailAccounts() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (list: MailAccount[]) => api.setMailAccounts(list),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: qk.mailAccounts() });
      void qc.invalidateQueries({ queryKey: ["config"] });
    },
  });
}

export function useTestMailAccount() {
  const api = useApi();
  return useMutation({ mutationFn: (a: MailAccount) => api.testMailAccount(a) });
}

// ---- shared contexts

export function useContexts() {
  const api = useApi();
  return useQuery({ queryKey: qk.contexts.list(), queryFn: () => api.contexts(), refetchInterval: 8000 });
}

export function useCreateContext() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (b: ContextCreate) => api.createContext(b),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: qk.contexts.all() });
      void qc.invalidateQueries({ queryKey: qk.sessions.all() });
    },
  });
}

export function useDeleteContext() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({ mutationFn: (id: string) => api.deleteContext(id), onSuccess: () => void qc.invalidateQueries({ queryKey: qk.contexts.all() }) });
}

// ---- email drafts

/** Every draft; the list is asked again every few seconds (the assistant may have written one). */
export function useEmailDrafts() {
  const api = useApi();
  return useQuery({ queryKey: qk.emailDrafts.list(), queryFn: () => api.emailDrafts(), refetchInterval: 5000 });
}

function useDraftMutation<V, R>(fn: (v: V) => Promise<R>) {
  const qc = useQueryClient();
  return useMutation({ mutationFn: fn, onSettled: () => void qc.invalidateQueries({ queryKey: qk.emailDrafts.all() }) });
}

export function useCreateDraft() {
  const api = useApi();
  return useDraftMutation((b: EmailDraftCreate) => api.createEmailDraft(b));
}
export function useUpdateDraft() {
  const api = useApi();
  return useDraftMutation((v: { id: string; patch: EmailDraftPatch }) => api.updateEmailDraft(v.id, v.patch));
}
export function useSendDraft() {
  const api = useApi();
  return useDraftMutation((v: { id: string; body: EmailDraftSend }) => api.sendEmailDraft(v.id, v.body));
}
export function useRejectDraft() {
  const api = useApi();
  return useDraftMutation((v: { id: string; reason?: string }) => api.rejectEmailDraft(v.id, v.reason));
}
export function useCancelDraft() {
  const api = useApi();
  return useDraftMutation((id: string) => api.cancelEmailDraft(id));
}
export function useDeleteDraft() {
  const api = useApi();
  return useDraftMutation((id: string) => api.deleteEmailDraft(id));
}
