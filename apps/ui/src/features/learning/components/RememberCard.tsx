import { useQueryClient } from "@tanstack/react-query";
import { errorMessage } from "../../../shared/lib/errors";
import { qk } from "../../../shared/query/keys";
import { Button, Icon } from "../../../shared/ui";
import type { Lesson } from "../../../shared/api/types";
import { useApproveLesson, useRejectLesson } from "../api";
import { rememberText } from "../model/lessons";
import "../learning.css";

/**
 * «Vuoi che lo ricordi?»: a memory lesson (after a thumbs down, or the reflection of the agent) shown in the conversation it
 * comes from. Conferma approves it (it becomes a memory the assistant always keeps in mind), Scarta drops it.
 */
export function RememberCard({ lesson }: { lesson: Lesson }) {
  const qc = useQueryClient();
  const approve = useApproveLesson();
  const reject = useRejectLesson();
  const busy = approve.isPending || reject.isPending;
  const failure = approve.isError ? approve.error : reject.isError ? reject.error : null;
  return (
    <div className="ln-remember" role="group" aria-label="Vuoi che lo ricordi?">
      <b><Icon name="bulb" size={16} /> Vuoi che lo ricordi?</b>
      <p className="ln-remember-text">{rememberText(lesson)}</p>
      {failure !== null && <p className="field-error" role="alert">{errorMessage(failure)}</p>}
      <div className="ln-actions">
        <Button size="sm" variant="primary" icon="check" disabled={busy} onClick={() => approve.mutate({ id: lesson.id }, { onSuccess: () => void qc.invalidateQueries({ queryKey: qk.memories.all() }) })}>Conferma</Button>
        <Button size="sm" variant="ghost" icon="close" disabled={busy} onClick={() => reject.mutate(lesson.id)}>Scarta</Button>
      </div>
    </div>
  );
}
