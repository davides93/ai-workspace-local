import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { qk } from "../../shared/query/keys";
import type { ChannelCreate, ChannelPatch, MailAccount } from "../../shared/api/types";

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
