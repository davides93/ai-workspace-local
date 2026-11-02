import { useState } from "react";
import { emptyRow, type KvRow } from "../lib/kvRows";
import { Button, IconButton } from "./Button";
import "./KeyValueEditor.css";

/**
 * Rows of name/value pairs (environment variables, HTTP headers). A row whose name `isSecret`
 * shows its value masked, with a button to reveal it.
 */
export function KeyValueEditor({ idPrefix, label, rows, onChange, keyLabel = "Nome", valueLabel = "Valore", keyPlaceholder, valuePlaceholder, isSecret, addLabel = "Aggiungi riga", emptyText }: {
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
  addLabel?: string;
  emptyText?: string;
}) {
  const [revealed, setRevealed] = useState<Set<string>>(new Set());
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
        const secret = !!isSecret?.(r.key);
        const shown = revealed.has(r.id);
        return (
          <div className="kve-row" key={r.id}>
            <input
              id={`${idPrefix}-k${i}`} className="input mono kve-key" aria-label={`${keyLabel} ${i + 1}`} placeholder={keyPlaceholder}
              value={r.key} autoComplete="off" spellCheck={false} onChange={(e) => patch(r.id, { key: e.target.value })}
            />
            <input
              id={`${idPrefix}-v${i}`} className="input mono kve-value" aria-label={`${valueLabel} ${i + 1}`} placeholder={valuePlaceholder}
              type={secret && !shown ? "password" : "text"} value={r.value} autoComplete="off" spellCheck={false}
              onChange={(e) => patch(r.id, { value: e.target.value })}
            />
            <div className="kve-actions">
              {secret && (
                <Button
                  size="sm" variant="ghost" aria-label={shown ? `Nascondi il valore ${i + 1}` : `Mostra il valore ${i + 1}`}
                  aria-pressed={shown} onClick={() => toggleReveal(r.id)}
                >
                  {shown ? "Nascondi" : "Mostra"}
                </Button>
              )}
              <IconButton aria-label={`Rimuovi la riga ${i + 1}`} icon="trash" size={16} onClick={() => onChange(rows.filter((x) => x.id !== r.id))} />
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
