import { useId } from "react";
import { Select } from "../../../shared/ui";
import type { StepDraft } from "../model/steps";

import { t } from "../../../shared/i18n";
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
        id={id} className="placeholder-select" value="" aria-label={t("workflows.inserisci_un_segnaposto")}
        onChange={(e) => {
          if (e.target.value) insert(e.target.value);
        }}
      >
        <option value="">{t("workflows.inserisci_segnaposto")}</option>
        {inputKeys.length > 0 && <optgroup label={t("workflows.input_del_workflow")}>{inputKeys.map((k) => <option key={k} value={`{{inputs.${k}}}`}>{`{{inputs.${k}}}`}</option>)}</optgroup>}
        {before.length > 0 && <optgroup label={t("workflows.risultato_dei_passi_precedenti")}>{before.map((s) => <option key={s.id} value={`{{steps.${s.id}.output}}`}>{`{{steps.${s.id}.output}}`}</option>)}</optgroup>}
        {before.length > 0 && <optgroup label={t("workflows.risultato_come_json_aggiungi_campo")}>{before.map((s) => <option key={s.id} value={`{{steps.${s.id}.json}}`}>{`{{steps.${s.id}.json}}`}</option>)}</optgroup>}
        {before.length > 0 && <optgroup label={t("workflows.errore_di_un_passo_che")}>{before.map((s) => <option key={s.id} value={`{{steps.${s.id}.error}}`}>{`{{steps.${s.id}.error}}`}</option>)}</optgroup>}
        <optgroup label={t("workflows.altro")}>
          <option value="{{now}}">{"{{now}}"} · {t("workflows.placeholder_now")}</option>
          <option value="{{inputs.payload}}">{"{{inputs.payload}}"} · {t("workflows.placeholder_payload")}</option>
        </optgroup>
      </Select>
      {quoted && <span className="faint text-xs">{t("workflows.nel_json_mettilo_tra_virgolette")}</span>}
    </div>
  );
}
