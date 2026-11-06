import { useState } from "react";
import { Field, Textarea } from "../../../shared/ui";
import { parseSchemaText, schemaToText } from "../model/agents";

/**
 * Optional JSON Schema of the final answer. The text is edited freely; the schema is stored in the draft only
 * while the text is valid JSON, and `onError` tells the editor when saving must wait.
 */
export function OutputSection({ agent, onChange, onError }: {
  agent: { output_schema?: unknown };
  onChange: (patch: { output_schema?: Record<string, unknown> }) => void;
  onError: (message: string | null) => void;
}) {
  const [text, setText] = useState(() => schemaToText(agent.output_schema));
  const [error, setError] = useState<string | null>(null);
  const edit = (next: string) => {
    setText(next);
    const r = parseSchemaText(next);
    setError(r.error ?? null);
    onError(r.error ?? null);
    if (!r.error) onChange({ output_schema: r.value });
  };
  return (
    <div className="form-grid">
      <Field
        label="Schema della risposta (JSON Schema)" htmlFor="ag-schema" error={error ?? undefined}
        hint="Facoltativo. Se lo imposti, la risposta finale dell'agente deve essere un JSON conforme (type, properties, required, items, enum, additionalProperties, minItems, maxItems); se non lo è, l'agente ha un tentativo per correggerla."
      >
        <Textarea
          id="ag-schema" rows={6} className="mono" value={text} onChange={(e) => edit(e.target.value)} spellCheck={false}
          placeholder={'{ "type": "object", "properties": { "esito": { "type": "string" } }, "required": ["esito"] }'} aria-invalid={!!error}
        />
      </Field>
    </div>
  );
}
