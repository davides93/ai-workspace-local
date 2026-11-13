import { useState } from "react";
import type { FieldSchema } from "../../../shared/api/types";
import { Input, Select, Toggle, TagInput } from "../../../shared/ui";
import { parseNumber } from "../model/config";

import { t } from "../../../shared/i18n";
/** Draft text of an input: follows the saved value whenever that changes (another layer, a reset, a save). */
function useDraft(shown: string) {
  const [draft, setDraft] = useState(shown);
  const [seen, setSeen] = useState(shown);
  if (shown !== seen) {
    setSeen(shown);
    setDraft(shown);
  }
  return [draft, setDraft] as const;
}

const text = (v: unknown) => (v === undefined || v === null ? "" : String(v));

function TextSetting({ id, field, value, onCommit }: { id: string; field: FieldSchema; value: unknown; onCommit: (v: unknown) => void }) {
  const shown = text(value);
  const [draft, setDraft] = useDraft(shown);
  const dirty = draft !== shown;
  return (
    <>
      <Input
        id={id} mono value={draft} autoComplete={field.secret ? "new-password" : "off"} spellCheck={false} placeholder={field.placeholder}
        type={field.secret ? "password" : "text"} onChange={(e) => setDraft(e.target.value)}
        onBlur={() => dirty && onCommit(draft)}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          else if (e.key === "Escape") setDraft(shown);
        }}
      />
      {dirty && <span className="hint">{t("settings.invio_per_salvare_esc_per")}</span>}
    </>
  );
}

function NumberSetting({ id, field, value, onCommit }: { id: string; field: FieldSchema; value: unknown; onCommit: (v: unknown) => void }) {
  const shown = text(value);
  const [draft, setDraft] = useDraft(shown);
  const parsed = parseNumber(draft, field);
  const dirty = draft !== shown;
  const step = field.step ?? (field.type === "integer" ? 1 : 0.01);
  const ranged = field.minimum !== undefined && field.maximum !== undefined && field.maximum - field.minimum <= 100000;
  const commit = (raw: string) => {
    const p = parseNumber(raw, field);
    if (p.ok && String(p.value) !== shown) onCommit(p.value);
  };
  return (
    <>
      <div className="range-row">
        {ranged && (
          <input
            type="range" aria-label={field.title} min={field.minimum} max={field.maximum} step={step}
            value={parsed.ok ? parsed.value : field.minimum} onChange={(e) => setDraft(e.target.value)}
            onPointerUp={(e) => commit(e.currentTarget.value)} onKeyUp={(e) => commit(e.currentTarget.value)}
          />
        )}
        <input
          id={id} className={`input mono num${ranged ? " input-num" : ""}`} inputMode="decimal" value={draft} aria-invalid={dirty && !parsed.ok}
          onChange={(e) => setDraft(e.target.value)} onBlur={() => dirty && commit(draft)}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
            else if (e.key === "Escape") setDraft(shown);
          }}
        />
      </div>
      {dirty && !parsed.ok && <span className="field-error" role="alert">{parsed.error}</span>}
    </>
  );
}

/** The editor of a setting, by its type. Switches, choices and lists save at once; text and numbers on Enter or when you leave the field. */
export function SettingControl({ id, field, value, onCommit, optionLabel = String }: {
  id: string;
  field: FieldSchema;
  value: unknown;
  onCommit: (v: unknown) => void;
  /** Readable name of an enum choice. */
  optionLabel?: (v: string | number) => string;
}) {
  const label = field.title ?? id;
  if (field.type === "boolean") return <Toggle id={id} label={label} checked={value === true} onChange={onCommit} />;
  if (field.enum) {
    return (
      <Select id={id} value={text(value)} onChange={(e) => onCommit(field.type === "string" ? e.target.value : Number(e.target.value))}>
        {field.enum.map((o) => <option key={String(o)} value={String(o)}>{optionLabel(o)}</option>)}
      </Select>
    );
  }
  if (field.type === "integer" || field.type === "number") return <NumberSetting id={id} field={field} value={value} onCommit={onCommit} />;
  if (field.type === "array") {
    return (
      <TagInput
        id={id} label={label} value={Array.isArray(value) ? value.map(String) : []} onChange={onCommit} unique liveSplit
        placeholder={field.placeholder ?? t("settings.scrivi_un_valore_e_premi")}
      />
    );
  }
  return <TextSetting id={id} field={field} value={value} onCommit={onCommit} />;
}
