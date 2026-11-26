import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { errorMessage } from "../../shared/lib/errors";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { TaskCreate } from "../../shared/api/types";

import { t } from "../../shared/i18n";
/** Starts a collaborator: a background job that goes on while the person is away. */
export function useCreateCollaborator(onCreated?: () => void) {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: TaskCreate) => api.createTask(body),
    meta: { handled: true },
    onSuccess: () => { toast(t("collab.started")); onCreated?.(); },
    onError: (e) => toast(t("collab.start_failed", { error: errorMessage(e) }), "crit"),
    onSettled: () => { void qc.invalidateQueries({ queryKey: qk.tasks.all() }); void qc.invalidateQueries({ queryKey: qk.sessions.all() }); },
  });
}
