import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import { useSystem } from "../../shared/query/system";

const MESH_POLL_MS = 8_000;

export function useMesh() {
  const api = useApi();
  return useQuery({ queryKey: qk.mesh(), queryFn: () => api.mesh(), refetchInterval: MESH_POLL_MS });
}

/** QR that pairs a phone in one step; only available once the device is reachable on the LAN. */
export function usePairingQr(address: string | null) {
  const api = useApi();
  const { data: system } = useSystem();
  return useQuery({
    queryKey: qk.pairingQr(address, system?.pairing_code),
    queryFn: () => api.pairingQr(),
    enabled: !!address,
    retry: false,
  });
}

export function usePairPeer() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { address: string; code: string }) => api.pairPeer(v.address, v.code),
    onSuccess: (p) => {
      toast(`Accoppiato con ${p.name}: i suoi modelli e agenti sono disponibili qui`);
      void qc.invalidateQueries({ queryKey: qk.mesh() });
      void qc.invalidateQueries({ queryKey: qk.system() });
      void qc.invalidateQueries({ queryKey: qk.runtimes.all() });
    },
  });
}

export function useRemovePeer() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (p: { id: string; name: string }) => api.removePeer(p.id),
    onSuccess: (_r, p) => {
      toast(`${p.name} rimosso`);
      void qc.invalidateQueries({ queryKey: qk.mesh() });
      void qc.invalidateQueries({ queryKey: qk.system() });
      void qc.invalidateQueries({ queryKey: qk.runtimes.all() });
    },
  });
}
