import { useId } from "react";
import { Select } from "../../../shared/ui";
import type { StepDraft } from "../model/steps";

/** Inserts a `{{…}}` placeholder where the cursor is in the field `targetId`. */
export function PlaceholderInsert({ targetId, value, onChange, before, inputKeys, quoted }: {
  targetId: string;
  value: string;
  onChange: (next: string) => void;
  /** Steps that run before this one: their output can be used. */
  before: StepDraft[];
  inputKeys: string[];
  /** The text is JSON: the placeholder is inserted as is, the author puts the quotes. */
  quoted?: boolean;
}) {
  const id = useId();
  const insert = (text: string) => {
    const el = document.getElementById(targetId) as HTMLInputElement | HTMLTextAreaElement | null;
    const start = el?.selectionStart ?? value.length;
    const end = el?.selectionEnd ?? start;
    onChange(value.slice(0, start) + text + value.slice(end));
    requestAnimationFrame(() => {
      el?.focus();
      el?.setSelectionRange(start + text.length, start + text.length);
    });
  };
  return (
    <div className="placeholder-insert">
      <Select
        id={id} className="placeholder-select" value="" aria-label="Inserisci un segnaposto"
        onChange={(e) => {
          if (e.target.value) insert(e.target.value);
        }}
      >
        <option value="">Inserisci segnaposto…</option>
        {inputKeys.length > 0 && <optgroup label="Input del workflow">{inputKeys.map((k) => <option key={k} value={`{{inputs.${k}}}`}>{`{{inputs.${k}}}`}</option>)}</optgroup>}
        {before.length > 0 && <optgroup label="Risultato dei passi precedenti">{before.map((s) => <option key={s.id} value={`{{steps.${s.id}.output}}`}>{`{{steps.${s.id}.output}}`}</option>)}</optgroup>}
        <optgroup label="Altro">
          <option value="{{now}}">{"{{now}}"} · data e ora</option>
          <option value="{{inputs.payload}}">{"{{inputs.payload}}"} · corpo del webhook</option>
        </optgroup>
      </Select>
      {quoted && <span className="faint text-xs">Nel JSON mettilo tra virgolette.</span>}
    </div>
  );
}
