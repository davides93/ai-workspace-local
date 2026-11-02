import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { ConfigView } from "../../shared/api/types";

export function useConfig() {
  const api = useApi();
  return useQuery({ queryKey: qk.config(), queryFn: () => api.config() });
}

export function useSetConfig() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { layer: "global" | "device" | "workspace"; key: string; value: unknown; layerLabel: string }) => api.setConfig(v.layer, v.key, v.value),
    onSuccess: (view: ConfigView, v) => {
      qc.setQueryData(qk.config(), view);
      toast(`${v.key} salvato nello strato ${v.layerLabel}`);
    },
  });
}
