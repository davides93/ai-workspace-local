import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { Lesson, LessonStatus } from "../../shared/api/types";
import { approvedMessage, withDecided } from "./model/lessons";

import { t } from "../../shared/i18n";
/** Lessons proposed by the agents; a new thumbs down can add one, so the list is refreshed now and then. */
export function useLessons(status?: LessonStatus) {
  const api = useApi();
  return useQuery({ queryKey: qk.lessons.list(status), queryFn: () => api.lessons(status), refetchInterval: 30_000 });
}

/** Keeps the cached lists in step with a decision, then asks for the truth. */
function useDecision<V>(run: (v: V) => Promise<Lesson>, message: (l: Lesson) => string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: run,
    // The reason (already decided, text too long…) is shown in the form, not as a toast.
    meta: { handled: true },
    onSuccess: (l) => {
      toast(message(l));
      for (const status of [undefined, "pending", "approved", "rejected"] as const) {
        qc.setQueryData<Lesson[]>(qk.lessons.list(status), (list) => (list ? withDecided(list, l, status === "pending") : list));
      }
      void qc.invalidateQueries({ queryKey: qk.lessons.all() });
    },
  });
}

export function useApproveLesson() {
  const api = useApi();
  return useDecision((v: { id: string; title?: string; content?: string }) => api.approveLesson(v.id, { title: v.title, content: v.content }), approvedMessage);
}

export function useRejectLesson() {
  const api = useApi();
  return useDecision((id: string) => api.rejectLesson(id), (l) => `Lezione scartata: «${l.title}»`);
}

/** Asks the agent for a lesson on a conversation now; the answer waits for the model, which can take a while. */
export function useReflect() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (sessionId: string) => api.reflectSession(sessionId),
    onSuccess: () => {
      toast(t("learning.ho_proposto_una_lezione_la"));
      void qc.invalidateQueries({ queryKey: qk.lessons.all() });
    },
  });
}
