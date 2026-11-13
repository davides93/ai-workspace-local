import { useRef, useState, type ClipboardEvent, type KeyboardEvent } from "react";
import { cx } from "../lib/cx";
import { splitList } from "../lib/tokens";
import { Icon } from "./Icon";
import "./TagInput.css";

import { t as tr } from "../i18n";
/**
 * List of text values edited as removable tags (arguments, domains).
 * Enter or leaving the field turns the typed text into tags, Backspace on an empty field removes
 * the last one, pasting several values adds them all. `parse` decides how a line splits: the
 * default splits on whitespace and commas; pass `splitShellWords` to keep spaces inside quotes.
 */
export function TagInput({ id, label, value, onChange, parse = splitList, liveSplit = false, unique = false, placeholder, mono = true, disabled, describedBy }: {
  /** Id of the text field, so a `<label htmlFor>` can name it. */
  id?: string;
  /** Accessible name of the whole group. */
  label: string;
  value: string[];
  onChange: (next: string[]) => void;
  parse?: (raw: string) => string[];
  /** Also commit while typing a space or a comma (only sensible with the default `parse`). */
  liveSplit?: boolean;
  /** Ignore values that are already in the list. */
  unique?: boolean;
  placeholder?: string;
  mono?: boolean;
  disabled?: boolean;
  describedBy?: string;
}) {
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  const add = (raw: string) => {
    const tokens = parse(raw);
    if (tokens.length === 0) return setDraft("");
    let next = [...value, ...tokens];
    if (unique) next = next.filter((t, i) => next.indexOf(t) === i);
    if (next.length !== value.length) onChange(next);
    setDraft("");
  };
  const removeAt = (i: number) => {
    onChange(value.filter((_, j) => j !== i));
    inputRef.current?.focus();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      if (draft.trim()) add(draft);
    } else if (e.key === "Backspace" && draft === "" && value.length > 0) {
      onChange(value.slice(0, -1));
    }
  };
  const onPaste = (e: ClipboardEvent<HTMLInputElement>) => {
    const text = e.clipboardData.getData("text");
    if (!/[\n\r]/.test(text) && parse(text).length < 2) return;
    e.preventDefault();
    add(text);
  };

  return (
    <div
      className={cx("taginput", mono && "mono", disabled && "is-disabled")}
      role="group"
      aria-label={label}
      onClick={(e) => e.target === e.currentTarget && inputRef.current?.focus()}
    >
      {value.map((t, i) => (
        <span className="ti-tag" key={`${i}-${t}`}>
          <span className="ti-text" title={t}>{t === "" ? "\"\"" : t}</span>
          <button type="button" className="ti-x" aria-label={tr("ui.rimuovi_2", { p: t === "" ? "argomento vuoto" : t })} disabled={disabled} onClick={() => removeAt(i)}>
            <Icon name="close" size={12} />
          </button>
        </span>
      ))}
      <input
        ref={inputRef}
        id={id}
        className="taginput-field"
        value={draft}
        placeholder={value.length === 0 ? placeholder : undefined}
        disabled={disabled}
        aria-describedby={describedBy}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => {
          const v = e.target.value;
          if (liveSplit && /[\s,;]$/.test(v) && v.trim()) add(v);
          else setDraft(v);
        }}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
        onBlur={() => draft.trim() && add(draft)}
      />
    </div>
  );
}
