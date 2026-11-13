import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { UpdateStatus } from "../../shared/api/types";

import { t } from "../../shared/i18n";
/** While an installer downloads the status is read every second; otherwise once a minute. */
const DOWNLOAD_POLL_MS = 1_000;
const IDLE_POLL_MS = 60_000;

/** Update status: current version, channel in use and what the last check found. */
export function useUpdateStatus() {
  const api = useApi();
  return useQuery({
    queryKey: qk.update(),
    queryFn: () => api.updateStatus(),
    refetchInterval: (q) => (q.state.data?.state === "downloading" ? DOWNLOAD_POLL_MS : IDLE_POLL_MS),
  });
}

function useUpdateAction(run: (api: ReturnType<typeof useApi>) => Promise<UpdateStatus>, done?: (s: UpdateStatus) => string | undefined) {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => run(api),
    onSuccess: (s) => {
      qc.setQueryData(qk.update(), s);
      const msg = done?.(s);
      if (msg) toast(msg);
    },
    // A failure is also in the status (`error`), so the panel shows it even after a reload.
    onError: () => void qc.invalidateQueries({ queryKey: qk.update() }),
  });
}

export const useCheckUpdate = () =>
  useUpdateAction((api) => api.checkUpdate(), (s) => (s.available ? t("settings.update_available", { version: s.available.version }) : t("settings.sei_aggiornato")));
export const useDownloadUpdate = () => useUpdateAction((api) => api.downloadUpdate());
export const useInstallUpdate = () => useUpdateAction((api) => api.installUpdate(), (s) => s.message ?? t("settings.installer_avviato"));
