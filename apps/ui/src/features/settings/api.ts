import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { BackgroundStatus, ConfigView } from "../../shared/api/types";
import { LAYER_LABEL, type WritableLayer } from "./model/config";

import { t } from "../../shared/i18n";
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
      toast(v.value === null ? t("settings.ripristinato_nello_strato", { title: v.title, layer: LAYER_LABEL[v.layer] }) : t("settings.salvato_nello_strato", { title: v.title, layer: LAYER_LABEL[v.layer] }));
    },
  });
}

/** The phone's side of the background checks (notification permission, what is scheduled). Desktop answers "not supported". */
export function useBackgroundStatus(enabled: boolean) {
  const api = useApi();
  return useQuery({
    queryKey: ["background"] as const,
    queryFn: () => (api.backgroundStatus ? api.backgroundStatus() : Promise.reject(new Error(t("settings.non_disponibile")))),
    enabled: enabled && api.backgroundStatus !== undefined,
    retry: false,
  });
}

export function useBackgroundActions() {
  const api = useApi();
  const qc = useQueryClient();
  const done = (s: BackgroundStatus) => qc.setQueryData(["background"], s);
  return {
    askNotifications: useMutation({ mutationFn: async () => (api.backgroundRequestNotifications ? api.backgroundRequestNotifications() : undefined), onSuccess: (s) => s && done(s) }),
    configure: useMutation({ mutationFn: async () => (api.backgroundConfigure ? api.backgroundConfigure() : undefined), onSuccess: (s) => s && done(s) }),
    checkNow: useMutation({
      mutationFn: async () => (api.backgroundCheckNow ? api.backgroundCheckNow() : undefined),
      onSuccess: (s) => {
        if (s) done(s);
        void qc.invalidateQueries({ queryKey: qk.config() });
        toast(t("settings.controllo_avviato"));
      },
    }),
  };
}
