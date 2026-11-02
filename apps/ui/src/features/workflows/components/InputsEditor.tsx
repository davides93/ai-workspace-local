import type { Dispatch, SetStateAction } from "react";
import { Button, Field, IconButton, Input } from "../../../shared/ui";
import { keyedOne, type Keyed } from "../../../shared/lib/keyedList";
import type { InputRow, Issue } from "../model/draft";

/** Default inputs of the workflow: `{{inputs.nome}}` in the steps. A webhook body or a manual run can override them. */
export function InputsEditor({ rows, setRows, issues }: {
  rows: Keyed<InputRow>[];
  setRows: Dispatch<SetStateAction<Keyed<InputRow>[]>>;
  issues: Issue[];
}) {
  const patch = (uid: number, p: Partial<InputRow>) => setRows((r) => r.map((x) => (x.uid === uid ? { uid, value: { ...x.value, ...p } } : x)));
  return (
    <div className="stack">
      {rows.length === 0 && <p className="faint text-sm">Nessun input. Aggiungine uno per rendere il workflow riutilizzabile (una cartella, un indirizzo…).</p>}
      {rows.length > 0 && (
        <ul className="input-list" aria-label="Input del workflow">
          {rows.map(({ uid, value: r }, i) => {
            const mine = issues.filter((x) => x.scope === "inputs" && x.step === i);
            return (
              <li key={uid} className="input-row">
                <Field label={`Nome dell'input ${i + 1}`} htmlFor={`in-${uid}-key`} className="input-key">
                  <Input id={`in-${uid}-key`} mono value={r.key} placeholder="cartella" spellCheck={false} onChange={(e) => patch(uid, { key: e.target.value })} />
                </Field>
                <Field label={`Valore predefinito dell'input ${i + 1}`} htmlFor={`in-${uid}-value`} className="input-value">
                  <Input id={`in-${uid}-value`} mono={r.json} value={r.value} placeholder="~/Download" spellCheck={false} onChange={(e) => patch(uid, { value: e.target.value })} />
                </Field>
                <label className="check-inline text-xs input-json" title="Numero, vero/falso, elenco o oggetto: il valore resta JSON invece di diventare testo">
                  <input type="checkbox" checked={r.json} onChange={(e) => patch(uid, { json: e.target.checked })} /> JSON
                </label>
                <IconButton aria-label={`Rimuovi l'input ${r.key || i + 1}`} icon="trash" size={16} onClick={() => setRows((all) => all.filter((x) => x.uid !== uid))} />
                {mine.map((x) => <p key={x.message} className="field-error input-error" role="alert">{x.message}</p>)}
              </li>
            );
          })}
        </ul>
      )}
      <div><Button size="sm" icon="plus" onClick={() => setRows((r) => [...r, keyedOne({ key: "", value: "", json: false })])}>Aggiungi input</Button></div>
    </div>
  );
}
