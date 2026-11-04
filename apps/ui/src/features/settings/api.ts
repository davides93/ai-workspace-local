import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { ConfigView } from "../../shared/api/types";
import { LAYER_LABEL, type WritableLayer } from "./model/config";

export function useConfig() {
  const api = useApi();
  return useQuery({ queryKey: qk.config(), queryFn: () => api.config() });
}

/**
 * Writes one setting in one layer. `value: null` removes the key from the layer (the setting goes
 * back to what the layers below, or the default, say). The server answers with the whole view.
 */
export function useSetConfig() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { layer: WritableLayer; key: string; value: unknown; title: string }) => api.setConfig(v.layer, v.key, v.value),
    onSuccess: (view: ConfigView, v) => {
      qc.setQueryData(qk.config(), view);
      // The voice settings (language, engine, model) change what the chat's microphone uses.
      if (v.key.startsWith("voice.")) void qc.invalidateQueries({ queryKey: qk.voice() });
      toast(v.value === null ? `«${v.title}» ripristinato nello strato ${LAYER_LABEL[v.layer]}` : `«${v.title}» salvato nello strato ${LAYER_LABEL[v.layer]}`);
    },
  });
}
