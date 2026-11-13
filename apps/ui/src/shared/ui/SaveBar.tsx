import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import { Button } from "./Button";
import { Led } from "./Pill";
import "./SaveBar.css";

import { t } from "../i18n";
/**
 * Sticky action bar of an editable form: says whether there are unsaved changes and offers
 * "Salva" / "Annulla modifiche". It stays at the bottom of the scrolling page so a long form
 * (tools, steps) never hides its own save button.
 */
export function SaveBar({ dirty, isNew = false, saving, onSave, onDiscard, canSave = true, blocker, saveLabel = "Salva", children, className }: {
  dirty: boolean;
  /** The thing does not exist yet: saving creates it, so it is possible even without edits. */
  isNew?: boolean;
  saving?: boolean;
  onSave: () => void;
  onDiscard: () => void;
  /** False while the form has errors. */
  canSave?: boolean;
  /** Why saving is not possible right now ("Correggi i campi evidenziati"). */
  blocker?: string;
  saveLabel?: string;
  /** Extra controls on the left (e.g. a delete button). */
  children?: ReactNode;
  className?: string;
}) {
  const pending = dirty || isNew;
  const state = dirty && !canSave && blocker ? blocker : isNew && !dirty ? "Non ancora creato" : dirty ? "Modifiche non salvate" : "Tutto salvato";
  return (
    <div className={cx("savebar", pending && "dirty", className)} role="region" aria-label={t("ui.salvataggio")}>
      <div className="savebar-state" role="status">
        <Led tone={pending ? "run" : "off"} small />
        <span>{state}</span>
      </div>
      <div className="savebar-actions">
        {children}
        <Button variant="ghost" onClick={onDiscard} disabled={!dirty || saving}>{t("ui.annulla_modifiche")}</Button>
        <Button variant="primary" onClick={onSave} disabled={!pending || !canSave || saving}>{saving ? t("ui.salvataggio_2") : saveLabel}</Button>
      </div>
    </div>
  );
}
