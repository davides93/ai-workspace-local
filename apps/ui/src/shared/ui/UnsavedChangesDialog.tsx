import type { UnsavedGuard } from "../lib/useUnsavedGuard";
import { Button } from "./Button";
import { Dialog } from "./Dialog";

import { t } from "../i18n";
/** Prompt shown by `useUnsavedGuard` when the user tries to leave a form with unsaved changes. */
export function UnsavedChangesDialog({ guard, subject, onSave, saving, canSave = true }: {
  guard: UnsavedGuard;
  /** What is being edited ("l'agente «Coder»"). */
  subject: string;
  /** Saves the changes; resolve `true` when they were stored so the navigation can go on. */
  onSave?: () => Promise<boolean>;
  saving?: boolean;
  /** False while the form has errors: the changes cannot be saved as they are. */
  canSave?: boolean;
}) {
  if (guard.pending === null) return null;
  const saveAndGo = async () => {
    if (!onSave) return;
    if (await onSave()) guard.go(guard.pending ?? "/");
  };
  return (
    <Dialog
      title={t("ui.modifiche_non_salvate")}
      size="sm"
      onClose={guard.stay}
      footer={<>
        <Button onClick={guard.stay} data-autofocus>{t("ui.continua_a_modificare")}</Button>
        <Button variant="danger" onClick={guard.leave}>{t("ui.scarta")}</Button>
        {onSave && <Button variant="primary" onClick={() => void saveAndGo()} disabled={saving || !canSave}>{t("ui.salva_e_continua")}</Button>}
      </>}
    >
      <p>{t("ui.hai_modifiche_non_salvate_per", { subject })}</p>
      {onSave && !canSave && <p className="faint text-sm">{t("ui.non_si_possono_salvare_cosi")}</p>}
    </Dialog>
  );
}
