import type { ReactNode } from "react";
import { errorMessage } from "../lib/errors";
import { Button } from "./Button";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";

/**
 * Confirmation of an action that cannot be undone (delete, discard). The failure of the action
 * stays visible inside the dialog (`error`), so a backend refusal such as "agente integrato" is
 * read where the decision was taken.
 */
export function DecisionDialog({ title, children, confirmLabel, cancelLabel = "Annulla", tone = "danger", icon, pending, error, onConfirm, onCancel }: {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  tone?: "danger" | "primary";
  icon?: string;
  pending?: boolean;
  error?: unknown;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Dialog
      title={title}
      size="sm"
      onClose={onCancel}
      footer={<>
        <Button onClick={onCancel} data-autofocus>{cancelLabel}</Button>
        <Button variant={tone} icon={icon} onClick={onConfirm} disabled={pending}>{confirmLabel}</Button>
      </>}
    >
      {children}
      {error !== undefined && error !== null && (
        <p className="field-error row" role="alert">
          <Icon name="alert" size={14} />
          {errorMessage(error)}
        </p>
      )}
    </Dialog>
  );
}
