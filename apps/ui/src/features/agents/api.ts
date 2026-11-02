import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { AgentProfile, InferenceProfile } from "../../shared/api/types";

export function useAgents() {
  const api = useApi();
  return useQuery({ queryKey: qk.agents(), queryFn: () => api.agents() });
}

export function useProfiles() {
  const api = useApi();
  return useQuery({ queryKey: qk.profiles(), queryFn: () => api.profiles() });
}

export function useSaveAgent() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (a: AgentProfile) => api.saveAgent(a),
    onSuccess: (a) => {
      toast(`Agente "${a.name}" salvato`);
      void qc.invalidateQueries({ queryKey: qk.agents() });
    },
  });
}

export function useDeleteAgent() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteAgent(id),
    onSuccess: () => {
      toast("Agente eliminato");
      void qc.invalidateQueries({ queryKey: qk.agents() });
    },
  });
}

export function useSaveProfile() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (p: InferenceProfile) => api.saveProfile(p),
    onSuccess: () => {
      toast("Profilo salvato");
      void qc.invalidateQueries({ queryKey: qk.profiles() });
    },
  });
}

export function useDeleteProfile() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteProfile(id),
    onSuccess: () => {
      toast("Profilo eliminato");
      void qc.invalidateQueries({ queryKey: qk.profiles() });
    },
  });
}
