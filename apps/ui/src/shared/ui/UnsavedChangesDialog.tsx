import type { UnsavedGuard } from "../lib/useUnsavedGuard";
import { Button } from "./Button";
import { Dialog } from "./Dialog";

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
      title="Modifiche non salvate"
      size="sm"
      onClose={guard.stay}
      footer={<>
        <Button onClick={guard.stay} data-autofocus>Continua a modificare</Button>
        <Button variant="danger" onClick={guard.leave}>Scarta</Button>
        {onSave && <Button variant="primary" onClick={() => void saveAndGo()} disabled={saving || !canSave}>Salva e continua</Button>}
      </>}
    >
      <p>Hai modifiche non salvate per {subject}. Se esci ora andranno perse.</p>
      {onSave && !canSave && <p className="faint text-sm">Non si possono salvare così come sono: correggi prima gli errori indicati nel modulo.</p>}
    </Dialog>
  );
}
