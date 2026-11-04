import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { ignoreNotFound } from "../../shared/lib/errors";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { McpServer, ToolInfo } from "../../shared/api/types";

export function useTools() {
  const api = useApi();
  return useQuery({ queryKey: qk.tools(), queryFn: () => api.tools() });
}

export function useMcpServers() {
  const api = useApi();
  return useQuery({ queryKey: qk.mcp(), queryFn: () => api.mcpServers() });
}

/**
 * Switches several tools (a whole group, or just one) and shows the new state at once: the switch
 * must not wait for the round trip. A failure restores the previous list (and shows the toast).
 */
export function useSetToolsEnabled() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { names: string[]; enabled: boolean }) => Promise.all(v.names.map((n) => api.setToolEnabled(n, v.enabled))),
    onMutate: async (v) => {
      await qc.cancelQueries({ queryKey: qk.tools() });
      const previous = qc.getQueryData<ToolInfo[]>(qk.tools());
      qc.setQueryData<ToolInfo[]>(qk.tools(), (old) => old?.map((t) => (v.names.includes(t.name) ? { ...t, enabled: v.enabled } : t)));
      return { previous };
    },
    onError: (_e, _v, ctx) => {
      if (ctx?.previous) qc.setQueryData(qk.tools(), ctx.previous);
    },
    onSettled: () => void qc.invalidateQueries({ queryKey: qk.tools() }),
  });
}

type SavePayload = Partial<McpServer> & { name: string; transport: "stdio" | "http" };

/**
 * Creates or updates a server (the id in the payload decides). The engine connects an enabled
 * server right away, so the result carries the connection state: a failure to connect is
 * reported in the toast, the details stay on the server card.
 */
export function useSaveMcpServer() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: SavePayload) => api.saveMcpServer(s),
    onSuccess: (s, v) => {
      if (s.enabled && s.status === "error") toast(`"${s.name}" salvato, ma non si collega${s.error ? `: ${s.error}` : ""}`, "crit");
      else toast(v.id ? `Server MCP "${s.name}" aggiornato` : `Server MCP "${s.name}" aggiunto`);
      void qc.invalidateQueries({ queryKey: qk.mcp() });
      void qc.invalidateQueries({ queryKey: qk.tools() });
    },
  });
}

export function useRemoveMcpServer() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    // 404: the server was already removed from another client, which is what was asked.
    mutationFn: (s: Pick<McpServer, "id" | "name">) => ignoreNotFound(api.removeMcpServer(s.id)),
    onSuccess: (_r, s) => {
      toast(`Server MCP "${s.name}" rimosso`);
      void qc.invalidateQueries({ queryKey: qk.mcp() });
      void qc.invalidateQueries({ queryKey: qk.tools() });
    },
  });
}
