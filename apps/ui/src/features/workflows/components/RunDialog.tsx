import { useState } from "react";
import { Button, Dialog, Field, Input } from "../../../shared/ui";
import type { WorkflowDef } from "../../../shared/api/types";

const asText = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v));

/** Asks for the inputs of a manual run, prefilled with the defaults of the workflow. */
export function RunDialog({ workflow, pending, onRun, onClose }: {
  workflow: WorkflowDef;
  pending: boolean;
  onRun: (inputs: Record<string, unknown>) => void;
  onClose: () => void;
}) {
  const keys = Object.keys(workflow.inputs);
  const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(keys.map((k) => [k, asText(workflow.inputs[k])])));
  const submit = () =>
    onRun(Object.fromEntries(keys.map((k) => {
      // A value that was not text stays what it was when the text still reads the same way.
      const original = workflow.inputs[k];
      if (typeof original === "string") return [k, values[k]];
      try {
        return [k, JSON.parse(values[k])];
      } catch {
        return [k, values[k]];
      }
    })));
  return (
    <Dialog
      title={`Esegui «${workflow.name}»`} size="sm" onClose={onClose}
      footer={<><Button onClick={onClose}>Annulla</Button><Button variant="primary" icon="play" onClick={submit} disabled={pending}>Esegui</Button></>}
    >
      <form className="stack stack-lg" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <p className="muted text-sm">Questi valori valgono solo per questa esecuzione.</p>
        {keys.map((k, i) => (
          <Field key={k} label={k} htmlFor={`run-in-${k}`}>
            <Input id={`run-in-${k}`} mono data-autofocus={i === 0 ? "" : undefined} value={values[k]} onChange={(e) => setValues((v) => ({ ...v, [k]: e.target.value }))} spellCheck={false} />
          </Field>
        ))}
        <button type="submit" className="sr-only" tabIndex={-1}>Esegui</button>
      </form>
    </Dialog>
  );
}
