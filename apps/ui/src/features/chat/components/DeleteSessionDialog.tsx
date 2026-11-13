import { Button, Dialog } from "../../../shared/ui";
import type { Session } from "../../../shared/api/types";
import { useDeleteSession } from "../api";

import { t } from "../../../shared/i18n";
/** Asks before a session and its conversation are removed for good. */
export function DeleteSessionDialog({ session, onClose, onDeleted }: { session: Session; onClose: () => void; onDeleted: (id: string) => void }) {
  const del = useDeleteSession();
  return (
    <Dialog
      title={t("chat.elimina_la_sessione")}
      size="sm"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose} data-autofocus>{t("chat.annulla")}</Button>
          <Button
            variant="danger"
            icon="trash"
            disabled={del.isPending}
            onClick={() => del.mutate(session.id, { onSuccess: () => { onDeleted(session.id); onClose(); } })}
          >
            {del.isPending ? t("chat.elimino") : t("chat.elimina")}
          </Button>
        </>
      }
    >
      <p>Vuoi eliminare <b>{session.title}</b>? La conversazione viene rimossa in modo definitivo.</p>
      {session.worktree && (
        <p className="muted text-sm">
          La sessione lavora nel worktree <code>{session.worktree.branch}</code>: viene rimosso insieme a lei e le modifiche non ancora applicate al workspace andranno perse.
        </p>
      )}
      {session.last_status === "running" && <p className="muted text-sm">{t("chat.l_ultimo_run_di_questa")}</p>}
    </Dialog>
  );
}
