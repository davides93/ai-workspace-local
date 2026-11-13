import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { ignoreNotFound } from "../../shared/lib/errors";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { McpServer, ToolInfo } from "../../shared/api/types";

import { t as tr } from "../../shared/i18n";
export function useTools() {
  const api = useApi();
  return useQuery({ queryKey: qk.tools(), queryFn: () => api.tools() });
}

export function useMcpServers() {
  const api = useApi();
  return useQuery({
    queryKey: qk.mcp(),
    queryFn: () => api.mcpServers(),
    // A server that is reconnecting or waiting for the sign-in changes by itself: look again soon.
    refetchInterval: (q) => (q.state.data?.some((s) => s.enabled && ["connecting", "reconnecting", "auth_required"].includes(s.status)) ? 3_000 : false),
  });
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
      if (s.enabled && (s.status === "unreachable" || s.status === "auth_required")) toast((s.error ? tr("capabilities.saved_unreachable_error", { name: s.name, error: s.error }) : tr("capabilities.salvato_ma_non_si_collega", { name: s.name })), "crit");
      else toast(v.id ? tr("capabilities.server_mcp_aggiornato", { name: s.name }) : tr("capabilities.server_mcp_aggiunto", { name: s.name }));
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
      toast(tr("capabilities.server_mcp_rimosso", { name: s.name }));
      void qc.invalidateQueries({ queryKey: qk.mcp() });
      void qc.invalidateQueries({ queryKey: qk.tools() });
    },
  });
}

export function useReconnectMcp() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: Pick<McpServer, "id" | "name">) => api.reconnectMcpServer(s.id),
    onSuccess: (s) => {
      if (s.status === "connected") toast(tr("capabilities.e_collegato", { name: s.name }));
      else toast((s.error ? tr("capabilities.unreachable_error", { name: s.name, error: s.error }) : tr("capabilities.non_si_collega", { name: s.name })), "crit");
    },
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: qk.mcp() });
      void qc.invalidateQueries({ queryKey: qk.tools() });
    },
  });
}

/** «Accedi»: the engine gives the page of the authorization server, which opens in the system browser; the list refreshes while the person is there. */
/** «Consenti ed esegui»: the person approves the exact command shown (the hash is what they saw). */
export function useApproveMcp() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: Pick<McpServer, "id" | "name" | "approval_hash">) => api.approveMcpServer(s.id, s.approval_hash ?? ""),
    onSuccess: (s) => {
      if (s.status === "connected") toast(tr("capabilities.e_consentito_e_collegato", { name: s.name }));
      else toast((s.error ? tr("capabilities.allowed_unreachable_error", { name: s.name, error: s.error }) : tr("capabilities.e_consentito_ma_non_si", { name: s.name })), "crit");
    },
    onError: (e) => toast(e instanceof Error ? e.message : tr("capabilities.non_e_stato_possibile_consentire"), "crit"),
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: qk.mcp() });
      void qc.invalidateQueries({ queryKey: qk.tools() });
    },
  });
}

export function useMcpSignIn() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: Pick<McpServer, "id" | "name">) => api.mcpOAuthStart(s.id),
    onSuccess: (r, s) => {
      if (r.authorization_url) {
        window.open(r.authorization_url, "_blank", "noopener,noreferrer");
        toast(tr("capabilities.completa_l_accesso_a_nel", { name: s.name }));
      } else toast(tr("capabilities.accesso_a_eseguito", { name: s.name }));
    },
    onError: (e) => toast(e instanceof Error ? e.message : tr("capabilities.accesso_non_riuscito"), "crit"),
    onSettled: () => void qc.invalidateQueries({ queryKey: qk.mcp() }),
  });
}

export function useMcpSignOut() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: Pick<McpServer, "id" | "name">) => api.mcpOAuthSignout(s.id),
    onSuccess: (_r, s) => toast(tr("capabilities.accesso_a_dimenticato", { name: s.name })),
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: qk.mcp() });
      void qc.invalidateQueries({ queryKey: qk.tools() });
    },
  });
}
