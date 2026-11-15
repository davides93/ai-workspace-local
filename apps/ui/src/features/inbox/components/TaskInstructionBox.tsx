import { useId, useState } from "react";
import { formatNumber } from "../../../shared/lib/format";
import { Button, Icon, Textarea } from "../../../shared/ui";
import { appendDictated, checkInstruction } from "../model/tasks";
import { useDictation } from "./useDictation";

import { t } from "../../../shared/i18n";
/**
 * «Istruzione»: what the person wants to say to a task. A dictated text goes into the box so that it is read (and
 * corrected) before it is sent; only then does it go, with `source: "voice"` for as long as the box was not emptied.
 */
export function TaskInstructionBox({ busy, disabled, hint, onSend }: {
  busy: boolean;
  /** The task is finished: nothing can be said to it any more. */
  disabled?: boolean;
  /** Replaces the usual hint under the box (why it is disabled). */
  hint?: string;
  onSend: (text: string, source: "user" | "voice", sent: () => void) => void;
}) {
  const id = useId();
  const [text, setText] = useState("");
  const [dictated, setDictated] = useState(false);
  const voice = useDictation((heard) => {
    setText((current) => appendDictated(current, heard));
    setDictated(true);
  });
  const check = checkInstruction(text);
  const count = t("inbox.task_chars", { n: formatNumber(check.count, 0), max: formatNumber(check.max, 0) });
  const send = () => {
    if (!check.ok || busy || disabled) return;
    onSend(text.trim(), dictated ? "voice" : "user", () => {
      setText("");
      setDictated(false);
    });
  };
  const micLabel = voice.state === "rec" ? t("inbox.task_dictate_stop") : voice.state === "busy" ? t("inbox.task_dictate_busy") : t("inbox.task_dictate");
  return (
    <form className="tk-say" onSubmit={(e) => { e.preventDefault(); send(); }}>
      <div className="field">
        <label htmlFor={`${id}-i`}>{t("inbox.task_instruction")}</label>
        <Textarea
          id={`${id}-i`} rows={2} value={text} disabled={disabled} placeholder={t("inbox.task_instruction_placeholder")}
          aria-invalid={check.over || undefined} aria-describedby={`${id}-h ${id}-c`}
          onChange={(e) => {
            setText(e.target.value);
            // Cleared by hand: what comes next is typed again.
            if (!e.target.value.trim()) setDictated(false);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              send();
            }
          }}
        />
      </div>
      <div className="tk-say-row">
        <span id={`${id}-h`} className="hint tk-say-hint">
          {disabled ? hint : dictated ? t("inbox.task_dictate_check") : hint ?? t("inbox.task_instruction_hint")}
        </span>
        <span id={`${id}-c`} className={`tk-count text-xs num${check.over ? " over" : ""}`}>
          {check.over && <Icon name="alert" size={12} />}
          {count}
        </span>
        {voice.supported && (
          <Button
            size="sm" icon={voice.state === "busy" ? "more" : "mic"} aria-pressed={voice.state === "rec"} aria-label={micLabel}
            title={!voice.available ? voice.detail : micLabel} disabled={disabled || voice.state === "busy"} onClick={voice.toggle}
          />
        )}
        <Button type="submit" size="sm" variant="primary" icon="send" disabled={disabled || busy || !check.ok}>{t("inbox.task_instruction_send")}</Button>
      </div>
      {check.over && <p className="field-error tk-over" role="alert">{t("inbox.task_instruction_too_long", { max: formatNumber(check.max, 0) })}</p>}
    </form>
  );
}
