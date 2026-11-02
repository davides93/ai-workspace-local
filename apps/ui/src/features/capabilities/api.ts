import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { McpServer } from "../../shared/api/types";

export function useTools() {
  const api = useApi();
  return useQuery({ queryKey: qk.tools(), queryFn: () => api.tools() });
}

export function useMcpServers() {
  const api = useApi();
  return useQuery({ queryKey: qk.mcp(), queryFn: () => api.mcpServers() });
}

export function useSetToolEnabled() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { name: string; enabled: boolean }) => api.setToolEnabled(v.name, v.enabled),
    onSuccess: () => void qc.invalidateQueries({ queryKey: qk.tools() }),
  });
}

export function useSaveMcpServer() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: Partial<McpServer> & { name: string; transport: "stdio" | "http" }) => api.saveMcpServer(s),
    onSuccess: (s) => {
      toast(`Server MCP "${s.name}" aggiunto`);
      void qc.invalidateQueries({ queryKey: qk.mcp() });
      void qc.invalidateQueries({ queryKey: qk.tools() });
    },
  });
}

export function useRemoveMcpServer() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.removeMcpServer(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: qk.mcp() });
      void qc.invalidateQueries({ queryKey: qk.tools() });
    },
  });
}
