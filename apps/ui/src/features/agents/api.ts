import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { AgentProfile, InferenceProfile } from "../../shared/api/types";

import { t } from "../../shared/i18n";
export function useAgents() {
  const api = useApi();
  return useQuery({ queryKey: qk.agents(), queryFn: () => api.agents() });
}

/** The statistics of the bandit router for an agent (`enabled` off: nothing to read). */
export function useAgentRouting(id: string, enabled: boolean) {
  const api = useApi();
  return useQuery({ queryKey: qk.agentRouting(id), queryFn: () => api.agentRouting(id), enabled });
}

export function useResetRouting() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.resetAgentRouting(id),
    onSuccess: (_void, id) => {
      toast(t("agents.statistiche_del_router_azzerate"));
      void qc.invalidateQueries({ queryKey: qk.agentRouting(id) });
    },
  });
}

export function useProfiles() {
  const api = useApi();
  return useQuery({ queryKey: qk.profiles(), queryFn: () => api.profiles() });
}

/** Tool catalogue (shared query key: the Capabilities page reads the same entry). */
export function useAgentTools() {
  const api = useApi();
  return useQuery({ queryKey: qk.tools(), queryFn: () => api.tools() });
}

/** Installed models, to offer the ones a runtime can serve in the profile editor. */
export function useInstalledModels() {
  const api = useApi();
  return useQuery({ queryKey: qk.models.installed(), queryFn: () => api.models() });
}

export function useRuntimeList() {
  const api = useApi();
  return useQuery({ queryKey: qk.runtimes.list(), queryFn: () => api.runtimes() });
}

/** Puts `item` in a cached list by id, so the screen shows the saved value before the refetch. */
function upsert<T extends { id: string }>(list: T[] | undefined, item: T): T[] {
  const all = list ?? [];
  return all.some((x) => x.id === item.id) ? all.map((x) => (x.id === item.id ? item : x)) : [...all, item];
}

export function useSaveAgent() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (a: AgentProfile) => api.saveAgent(a),
    onSuccess: (a) => {
      toast(t("agents.agente_salvato", { name: a.name }));
      qc.setQueryData<AgentProfile[]>(qk.agents(), (list) => upsert(list, a));
      void qc.invalidateQueries({ queryKey: qk.agents() });
    },
  });
}

/** Failures (409 for a built-in agent or one used by a workflow) are shown inside the confirm dialog. */
export function useDeleteAgent() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteAgent(id),
    meta: { handled: true },
    onSuccess: (_void, id) => {
      toast(t("agents.agente_eliminato"));
      qc.setQueryData<AgentProfile[]>(qk.agents(), (list) => list?.filter((a) => a.id !== id));
      void qc.invalidateQueries({ queryKey: qk.agents() });
    },
  });
}

export function useSaveProfile() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (p: InferenceProfile) => api.saveProfile(p),
    onSuccess: (p) => {
      toast(t("agents.profilo_salvato", { name: p.name }));
      qc.setQueryData<InferenceProfile[]>(qk.profiles(), (list) => upsert(list, p));
      void qc.invalidateQueries({ queryKey: qk.profiles() });
    },
  });
}

/** 409 when an agent or a workflow still uses the profile: the message is shown inside the dialog. */
export function useDeleteProfile() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteProfile(id),
    meta: { handled: true },
    onSuccess: (_void, id) => {
      toast(t("agents.profilo_eliminato"));
      qc.setQueryData<InferenceProfile[]>(qk.profiles(), (list) => list?.filter((p) => p.id !== id));
      void qc.invalidateQueries({ queryKey: qk.profiles() });
    },
  });
}
