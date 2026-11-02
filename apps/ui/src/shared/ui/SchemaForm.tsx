import type { FieldSchema, SettingsSchema } from "../api/types";
import { Toggle } from "./Toggle";

// Generated settings form (JSON Schema subset).
export function SchemaForm({ schema, value, onChange, showAdvanced, idPrefix }: {
  schema: SettingsSchema;
  value: Record<string, unknown>;
  onChange: (v: Record<string, unknown>) => void;
  showAdvanced?: boolean;
  idPrefix: string;
}) {
  const entries = Object.entries(schema.properties).filter(([, f]) => showAdvanced || !f.advanced);
  if (entries.length === 0) return <p className="muted">Questo componente non ha impostazioni.</p>;
  const set = (k: string, v: unknown) => onChange({ ...value, [k]: v });
  return (
    <div className="form-grid">
      {entries.map(([key, f]) => (
        <SchemaField key={key} id={`${idPrefix}-${key}`} name={key} field={f} value={value[key] ?? f.default} onChange={(v) => set(key, v)} />
      ))}
    </div>
  );
}

function SchemaField({ id, name, field, value, onChange }: { id: string; name: string; field: FieldSchema; value: unknown; onChange: (v: unknown) => void }) {
  const label = field.title ?? name;
  const hint = field.description;
  if (field.type === "boolean") {
    return (
      <div className="row row-between nowrap">
        <div className="field">
          <span id={`${id}-label`} className="field-label">{label}</span>
          {hint && <span className="hint">{hint}</span>}
        </div>
        <Toggle id={id} checked={!!value} onChange={onChange} label={label} />
      </div>
    );
  }
  if (field.enum) {
    return (
      <div className="field">
        <label htmlFor={id}>{label}</label>
        <select id={id} className="input" value={String(value ?? "")} onChange={(e) => onChange(field.type === "string" ? e.target.value : Number(e.target.value))}>
          {field.enum.map((o) => <option key={String(o)} value={String(o)}>{String(o)}</option>)}
        </select>
        {hint && <span className="hint">{hint}</span>}
      </div>
    );
  }
  if (field.type === "integer" || field.type === "number") {
    const hasRange = field.minimum !== undefined && field.maximum !== undefined && field.maximum - field.minimum <= 100000;
    const n = typeof value === "number" ? value : Number(value ?? field.default ?? 0);
    return (
      <div className="field">
        <label htmlFor={id}>{label}</label>
        <div className="range-row">
          {hasRange && (
            <input type="range" aria-label={label} min={field.minimum} max={field.maximum} step={field.step ?? (field.type === "integer" ? 1 : 0.01)} value={n}
              onChange={(e) => onChange(field.type === "integer" ? parseInt(e.target.value, 10) : parseFloat(e.target.value))} />
          )}
          <input id={id} className={`input num${hasRange ? " input-num" : ""}`} type="number" min={field.minimum} max={field.maximum} step={field.step ?? (field.type === "integer" ? 1 : 0.01)} value={n}
            onChange={(e) => onChange(field.type === "integer" ? parseInt(e.target.value || "0", 10) : parseFloat(e.target.value || "0"))} />
        </div>
        {hint && <span className="hint">{hint}</span>}
      </div>
    );
  }
  if (field.type === "array") {
    const arr = Array.isArray(value) ? (value as string[]) : [];
    return (
      <div className="field">
        <label htmlFor={id}>{label}</label>
        <input id={id} className="input mono" placeholder={field.placeholder ?? "valori separati da spazio"} value={arr.join(" ")}
          onChange={(e) => onChange(e.target.value.split(/\s+/).filter(Boolean))} />
        {hint && <span className="hint">{hint}</span>}
      </div>
    );
  }
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <input id={id} className="input" type={field.secret ? "password" : "text"} placeholder={field.placeholder} value={String(value ?? "")} onChange={(e) => onChange(e.target.value)} />
      {hint && <span className="hint">{hint}</span>}
    </div>
  );
}
