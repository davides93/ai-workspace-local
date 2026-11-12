import { useId, useState } from "react";
import { Button, Icon, IconButton, Pill, Select, Textarea } from "../../../shared/ui";
import { MAX_MEMORY_CHARS, type Memory, type MemoryKind, type MemoryPatch } from "../../../shared/api/types";
import { dueLabel, KIND_LABEL, KIND_ORDER } from "../model/memories";

/**
 * One memory: its text, "Ricorda sempre" (pinned: it goes into every conversation), edit in place and forget.
 * A proposal (`status: "proposed"`) shows Conferma and Scarta instead.
 */
export function MemoryItem({ memory, busy, onSave, onForget, onApprove, onReject }: {
  memory: Memory;
  busy?: boolean;
  onSave: (patch: MemoryPatch) => void;
  onForget: () => void;
  onApprove: () => void;
  onReject: () => void;
}) {
  const id = useId();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(memory.text);
  const [kind, setKind] = useState<MemoryKind>(memory.kind);
  const proposed = memory.status === "proposed";
  const due = memory.kind === "reminder" ? dueLabel(memory.due_at) : undefined;
  const clean = text.trim();
  const valid = clean.length > 0 && [...clean].length <= MAX_MEMORY_CHARS;

  const start = () => {
    setText(memory.text);
    setKind(memory.kind);
    setEditing(true);
  };
  const save = () => {
    if (!valid) return;
    const patch: MemoryPatch = {};
    if (clean !== memory.text) patch.text = clean;
    if (kind !== memory.kind) patch.kind = kind;
    if (Object.keys(patch).length > 0) onSave(patch);
    setEditing(false);
  };

  if (editing) {
    return (
      <li className="th-mem editing">
        <label className="sr-only" htmlFor={`${id}-t`}>Testo del ricordo</label>
        <Textarea id={`${id}-t`} rows={3} value={text} maxLength={MAX_MEMORY_CHARS} autoFocus onChange={(e) => setText(e.target.value)} />
        <div className="th-mem-actions">
          <label className="sr-only" htmlFor={`${id}-k`}>Di cosa parla</label>
          <Select id={`${id}-k`} value={kind} onChange={(e) => setKind(e.target.value as MemoryKind)}>
            {KIND_ORDER.map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
          </Select>
          <Button size="sm" variant="primary" icon="check" disabled={!valid} onClick={save}>Salva</Button>
          <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>Annulla</Button>
        </div>
      </li>
    );
  }
  return (
    <li className={`th-mem${proposed ? " proposed" : ""}`}>
      <p className="th-mem-text">
        {memory.pinned && <span className="th-pin" title="Ricordo sempre questa cosa"><Icon name="bolt" size={13} /></span>}
        {memory.text}
      </p>
      {(due || proposed) && (
        <p className="th-mem-meta">
          {due && <Pill tone={due === "scaduto" ? "warn" : "info"}>{due}</Pill>}
          {proposed && <span className="faint text-xs">L'ho dedotto io: lo tengo solo se mi dici di sì.</span>}
        </p>
      )}
      <div className="th-mem-actions">
        {proposed ? (
          <>
            <Button size="sm" variant="primary" icon="check" disabled={busy} onClick={onApprove}>Conferma</Button>
            <Button size="sm" variant="ghost" icon="close" disabled={busy} onClick={onReject}>Scarta</Button>
          </>
        ) : (
          <>
            <Button size="sm" variant={memory.pinned ? "primary" : "ghost"} aria-pressed={memory.pinned} disabled={busy} onClick={() => onSave({ pinned: !memory.pinned })}>
              Ricorda sempre
            </Button>
            <IconButton aria-label="Modifica il ricordo" icon="edit" size={16} disabled={busy} onClick={start} />
            <IconButton aria-label="Dimentica" icon="trash" size={16} disabled={busy} onClick={onForget} />
          </>
        )}
      </div>
    </li>
  );
}
