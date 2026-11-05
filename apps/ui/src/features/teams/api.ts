import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { TeamDef } from "../../shared/api/types";
import { useAgents } from "../agents/api";
import { useRemoteAgents } from "../integrations/api";
import { localOptions, remoteOptions, type AgentOption } from "./model/teams";

export function useTeams() {
  const api = useApi();
  return useQuery({ queryKey: qk.teams(), queryFn: () => api.teams() });
}

/** The agents a team can use as supervisor, member or aggregator. */
export function useAgentOptions(): { options: AgentOption[]; loaded: boolean } {
  const agents = useAgents();
  const remotes = useRemoteAgents();
  const options = useMemo(() => [...localOptions(agents.data ?? []), ...remoteOptions(remotes.data ?? [])], [agents.data, remotes.data]);
  return { options, loaded: !!agents.data };
}

/** Puts `item` in a cached list by id, so the screen shows the saved value before the refetch. */
function upsert<T extends { id: string }>(list: T[] | undefined, item: T): T[] {
  const all = list ?? [];
  return all.some((x) => x.id === item.id) ? all.map((x) => (x.id === item.id ? item : x)) : [...all, item];
}

export function useSaveTeam() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (t: TeamDef) => api.saveTeam(t),
    onSuccess: (t) => {
      toast(`Team "${t.name}" salvato`);
      qc.setQueryData<TeamDef[]>(qk.teams(), (list) => upsert(list, t));
      void qc.invalidateQueries({ queryKey: qk.teams() });
    },
  });
}

/** The failure (if any) is shown inside the confirm dialog. */
export function useDeleteTeam() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.deleteTeam(id),
    meta: { handled: true },
    onSuccess: (_void, id) => {
      toast("Team eliminato");
      qc.setQueryData<TeamDef[]>(qk.teams(), (list) => list?.filter((t) => t.id !== id));
      void qc.invalidateQueries({ queryKey: qk.teams() });
    },
  });
}
