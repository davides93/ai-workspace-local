import { useEffect, useState } from "react";
import { cx } from "../lib/cx";
import { emptyRow, isKeptRow, renameRow, type KvRow } from "../lib/kvRows";
import { Button, IconButton } from "./Button";
import "./KeyValueEditor.css";

import { t, t as tr } from "../i18n";
/**
 * Rows of name/value pairs (environment variables, HTTP headers). A row whose name `isSecret`
 * shows its value masked, with a button to reveal it.
 *
 * With `mask`, a row that stands for a secret the server keeps (`KvRow.kept`, value still the mask) is a
 * read-only masked field with a "Cambia" button: there is nothing to reveal, and saving sends the mask back
 * unchanged so the stored value stays. After a change, "Mantieni" puts the mask back.
 */
export function KeyValueEditor({ idPrefix, label, rows, onChange, keyLabel = "Nome", valueLabel = "Valore", keyPlaceholder, valuePlaceholder, isSecret, mask, addLabel = "Aggiungi riga", emptyText }: {
  idPrefix: string;
  /** Accessible name of the group. */
  label: string;
  rows: KvRow[];
  onChange: (rows: KvRow[]) => void;
  keyLabel?: string;
  valueLabel?: string;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  isSecret?: (key: string) => boolean;
  /** What the server sends instead of a secret value (and accepts back as "keep it"). */
  mask?: string;
  addLabel?: string;
  emptyText?: string;
}) {
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
  // "Cambia" empties the value: the field takes the focus once it is editable.
  const [focusRow, setFocusRow] = useState<string | null>(null);
  useEffect(() => {
    if (focusRow === null) return;
    const i = rows.findIndex((r) => r.id === focusRow);
    if (i >= 0) document.getElementById(`${idPrefix}-v${i}`)?.focus();
    setFocusRow(null);
  }, [focusRow, rows, idPrefix]);
  const patch = (id: string, p: Partial<KvRow>) => onChange(rows.map((r) => (r.id === id ? { ...r, ...p } : r)));
  const toggleReveal = (id: string) =>
    setRevealed((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <div className="kve" role="group" aria-label={label}>
      {rows.length === 0 && emptyText && <p className="faint text-sm">{emptyText}</p>}
      {rows.map((r, i) => {
        const kept = mask !== undefined && isKeptRow(r, mask);
        const restorable = mask !== undefined && !kept && r.kept !== undefined && r.kept === r.key.trim();
        const secret = !!isSecret?.(r.key) || kept;
        const shown = revealed.has(r.id) && !kept;
        return (
          <div className="kve-row" key={r.id}>
            <input
              id={`${idPrefix}-k${i}`} className="input mono kve-key" aria-label={`${keyLabel} ${i + 1}`} placeholder={keyPlaceholder}
              value={r.key} autoComplete="off" spellCheck={false}
              onChange={(e) => onChange(rows.map((x) => (x.id === r.id ? renameRow(x, e.target.value, mask) : x)))}
            />
            <input
              id={`${idPrefix}-v${i}`} className={cx("input mono kve-value", kept && "kve-kept")} placeholder={valuePlaceholder}
              aria-label={kept ? t("ui.nascosto_resta_invariato", { valueLabel, p: i + 1 }) : `${valueLabel} ${i + 1}`}
              type={secret && !shown ? "password" : "text"} value={r.value} readOnly={kept} autoComplete="off" spellCheck={false}
              onChange={(e) => patch(r.id, { value: e.target.value })}
            />
            <div className="kve-actions">
              {kept && (
                <Button size="sm" variant="ghost" aria-label={tr("ui.cambia_il_valore", { number: i + 1 })} onClick={() => { patch(r.id, { value: "" }); setFocusRow(r.id); }}>
                  {t("ui.cambia")}</Button>
              )}
              {restorable && mask !== undefined && (
                <Button size="sm" variant="ghost" aria-label={tr("ui.mantieni_il_valore_salvato", { number: i + 1 })} onClick={() => patch(r.id, { value: mask })}>
                  {t("ui.mantieni")}</Button>
              )}
              {secret && !kept && (
                <Button
                  size="sm" variant="ghost" aria-label={shown ? t("ui.nascondi_il_valore", { p: i + 1 }) : t("ui.mostra_il_valore", { p: i + 1 })}
                  aria-pressed={shown} onClick={() => toggleReveal(r.id)}
                >
                  {shown ? t("ui.nascondi") : t("ui.mostra")}
                </Button>
              )}
              <IconButton aria-label={t("ui.rimuovi_la_riga", { p: i + 1 })} icon="trash" size={16} onClick={() => onChange(rows.filter((x) => x.id !== r.id))} />
            </div>
          </div>
        );
      })}
      <div>
        <Button size="sm" icon="plus" onClick={() => onChange([...rows, emptyRow()])}>{addLabel}</Button>
      </div>
    </div>
  );
}
