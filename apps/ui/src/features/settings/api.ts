import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { BackgroundStatus, ConfigView, FoldersAdded, FoldersInfo } from "../../shared/api/types";
import { errorMessage } from "../../shared/lib/errors";
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

/** The folders of the computer handed to the agents besides the workspace (the Mac App Store build; empty elsewhere). */
export function useFolders(enabled: boolean) {
  const api = useApi();
  return useQuery({ queryKey: qk.folders(), queryFn: () => api.folders(), enabled });
}

/**
 * Add and take off folders. Only the native app can: the macOS panel and the permission that lasts belong to the shell,
 * and the API has no route that changes the list (so a model cannot widen its own access).
 */
export function useFolderActions() {
  const api = useApi();
  const qc = useQueryClient();
  const set = (info: FoldersInfo) => {
    qc.setQueryData(qk.folders(), { supported: info.supported, folders: info.folders } satisfies FoldersInfo);
    // The agents' tools were handed the folders: nothing else of the snapshot changes, but the hint of the chat follows it.
    void qc.invalidateQueries({ queryKey: qk.system() });
  };
  return {
    add: useMutation({
      mutationFn: (): Promise<FoldersAdded> => {
        if (!api.addFolders) throw new Error(t("settings.cartelle_solo_app"));
        return api.addFolders(t("settings.cartelle_pannello_messaggio"), t("settings.cartelle_pannello_pulsante"));
      },
      onSuccess: (r) => {
        set(r);
        if (r.added > 0) toast(t("settings.cartelle_aggiunte", { n: r.added }));
      },
      onError: (e) => toast(errorMessage(e), "crit"),
    }),
    remove: useMutation({
      mutationFn: (v: { id: string; name: string }) => {
        if (!api.removeFolder) throw new Error(t("settings.cartelle_solo_app"));
        return api.removeFolder(v.id);
      },
      onSuccess: (info, v) => {
        set(info);
        toast(t("settings.cartella_tolta", { name: v.name }));
      },
      onError: (e) => toast(errorMessage(e), "crit"),
    }),
  };
}
