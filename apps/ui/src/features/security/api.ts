import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { ApiTokenCreate, HookConfig, PolicyRule } from "../../shared/api/types";

/** Tool catalogue (same query as the Capabilities page): names and risks for the rule tester. */
export function useToolList() {
  const api = useApi();
  return useQuery({ queryKey: qk.tools(), queryFn: () => api.tools() });
}

export function usePolicy() {
  const api = useApi();
  return useQuery({ queryKey: qk.policy(), queryFn: () => api.policy() });
}

export function useSetPolicy() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (rules: PolicyRule[]) => api.setPolicy(rules),
    onSuccess: (doc) => {
      qc.setQueryData(qk.policy(), doc);
      toast("Regole di policy salvate");
    },
  });
}

export function useHooks() {
  const api = useApi();
  return useQuery({ queryKey: qk.hooks(), queryFn: () => api.hooks() });
}

export function useSetHooks() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (hooks: HookConfig[]) => api.setHooks(hooks),
    onSuccess: (doc) => {
      qc.setQueryData(qk.hooks(), doc);
      toast("Hook salvati");
    },
  });
}

export function useApiTokens() {
  const api = useApi();
  return useQuery({ queryKey: qk.tokens(), queryFn: () => api.apiTokens() });
}

export function useCreateToken() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (req: ApiTokenCreate) => api.createApiToken(req),
    onSuccess: () => void qc.invalidateQueries({ queryKey: qk.tokens() }),
  });
}

export function useRevokeToken() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (t: { id: string; name: string }) => api.revokeApiToken(t.id),
    onSuccess: (_r, t) => {
      toast(`«${t.name}» revocato: non funziona più`);
      void qc.invalidateQueries({ queryKey: qk.tokens() });
    },
  });
}

export function useRotateToken() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (t: { id: string; name: string }) => api.rotateApiToken(t.id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: qk.tokens() }),
  });
}

export function useTlsStatus() {
  const api = useApi();
  return useQuery({ queryKey: qk.tls(), queryFn: () => api.tlsStatus() });
}
