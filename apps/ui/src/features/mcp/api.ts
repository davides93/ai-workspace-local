import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { McpElicitationAnswer } from "../../shared/api/types";

const ELICITATION_POLL_MS = 2_500;

/** The prompts of the connected MCP servers (only asked when `enabled`: developer mode). */
export function useMcpPrompts(enabled: boolean) {
  const api = useApi();
  return useQuery({ queryKey: qk.mcpPrompts(), queryFn: () => api.mcpPrompts(), enabled, staleTime: 30_000 });
}

/** Questions that MCP servers asked the person during a run. */
export function useMcpElicitations() {
  const api = useApi();
  return useQuery({ queryKey: qk.mcpElicitations(), queryFn: () => api.mcpElicitations(), refetchInterval: ELICITATION_POLL_MS });
}

export function useRespondMcpElicitation() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { id: string; answer: McpElicitationAnswer }) => api.respondMcpElicitation(v.id, v.answer),
    onError: (e) => toast(e instanceof Error ? e.message : "Risposta non inviata", "crit"),
    onSettled: () => void qc.invalidateQueries({ queryKey: qk.mcpElicitations() }),
  });
}
