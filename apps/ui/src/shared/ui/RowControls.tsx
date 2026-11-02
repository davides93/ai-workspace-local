import { useId, useLayoutEffect, useState, type ReactNode } from "react";
import { IconButton } from "./Button";
import "./RowControls.css";

/**
 * "Sposta su", "Sposta giù" and "Rimuovi" of a row of an editable list. After a move the focus stays
 * on the same button (or its twin when the row reached an end), so a row can be pushed several
 * places with the keyboard. The row must be keyed by a stable id, not by its position.
 */
export function RowControls({ index, count, noun, onMove, onRemove, children }: {
  index: number;
  count: number;
  /** Accessible name of the row: "la regola 3", "il passo scan". */
  noun: string;
  onMove: (delta: -1 | 1) => void;
  onRemove: () => void;
  /** Extra buttons placed before the standard ones. */
  children?: ReactNode;
}) {
  const id = useId();
  const upId = `${id}-up`;
  const downId = `${id}-down`;
  const [moved, setMoved] = useState<{ dir: -1 | 1; n: number } | null>(null);
  useLayoutEffect(() => {
    if (!moved) return;
    const pick = (dir: -1 | 1) => {
      const el = document.getElementById(dir < 0 ? upId : downId);
      return el instanceof HTMLButtonElement && !el.disabled ? el : null;
    };
    (pick(moved.dir) ?? pick(moved.dir === 1 ? -1 : 1))?.focus();
  }, [moved, upId, downId]);
  const move = (dir: -1 | 1) => {
    onMove(dir);
    setMoved((m) => ({ dir, n: (m?.n ?? 0) + 1 }));
  };
  return (
    <div className="row-controls">
      {children}
      <IconButton id={upId} aria-label={`Sposta su ${noun}`} icon="down" className="flip" size={16} disabled={index === 0} onClick={() => move(-1)} />
      <IconButton id={downId} aria-label={`Sposta giù ${noun}`} icon="down" size={16} disabled={index === count - 1} onClick={() => move(1)} />
      <IconButton aria-label={`Rimuovi ${noun}`} icon="trash" size={16} onClick={onRemove} />
    </div>
  );
}
