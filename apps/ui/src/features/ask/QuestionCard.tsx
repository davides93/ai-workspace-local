import { useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { cx } from "../../shared/lib/cx";
import { errorMessage, isNotFound } from "../../shared/lib/errors";
import { qk } from "../../shared/query/keys";
import type { Question, QuestionReply } from "../../shared/api/types";
import { Button, Field, Icon, Input, Select } from "../../shared/ui";
import type { FormValues } from "../mcp/model";
import type { QuestionItem } from "../chat/model/items";
import { choiceCount, choiceReply, closedText, confirmReply, digitChoice, emptyChoice, formReply, initialFormValues, isTypingTarget, pickChoice, questionTitle, typeOther, type ChoiceState } from "./model";
import { useAnnounce, useFocusOnOpen } from "./useAskFocus";
import "./ask.css";

import { t } from "../../shared/i18n";
/**
 * A question of the agent, as a row of the conversation: a card to answer while it waits, one quiet line with the answer once
 * it is closed (also when the conversation is loaded again from the stored messages).
 */
export function QuestionCard({ it }: { it: QuestionItem }) {
  return it.outcome === undefined ? <OpenQuestion q={it.q} /> : <ClosedQuestion it={it} outcome={it.outcome} />;
}

function ClosedQuestion({ it, outcome }: { it: QuestionItem; outcome: NonNullable<QuestionItem["outcome"]> }) {
  const heading = it.q.title?.trim() || it.q.message;
  return (
    <div className={cx("ask ask-closed", outcome === "answered" && "is-ok")}>
      <Icon name={outcome === "answered" ? "check" : "close"} size={15} />
      <div className="ask-closed-text">
        <span className="ask-closed-q">{heading}</span>
        <span className="ask-closed-a">{closedText(it.q, outcome, it.answer ?? null)}</span>
      </div>
    </div>
  );
}

type Sending = { status: "idle" } | { status: "sending" } | { status: "sent" } | { status: "gone" };

function OpenQuestion({ q }: { q: Question }) {
  const api = useApi();
  const qc = useQueryClient();
  const uid = useId();
  const root = useRef<HTMLFormElement>(null);
  const inputs = useRef<(HTMLInputElement | null)[]>([]);
  const [choice, setChoice] = useState<ChoiceState>(emptyChoice);
  const [values, setValues] = useState<FormValues>(() => initialFormValues(q.fields));
  const [state, setState] = useState<Sending>({ status: "idle" });
  const [error, setError] = useState<string | null>(null);
  const said = useAnnounce(t("ask.l_assistente_ha_una_domanda_testo", { message: q.message }));
  useFocusOnOpen(root);
  const busy = state.status !== "idle";
  const titleId = `${uid}-t`;
  const messageId = `${uid}-m`;
  const hintId = `${uid}-h`;
  const errorId = `${uid}-e`;

  /** Sends a reply; a refused answer keeps the question open with the reason, an answer that arrived after the end says so. */
  const send = async (reply: QuestionReply) => {
    if (busy) return;
    setError(null);
    setState({ status: "sending" });
    try {
      await api.answerQuestion(q.id, reply);
      // The row closes when the engine says so (`aiw.question_resolved`); until then the card stays, disabled.
      setState({ status: "sent" });
    } catch (e) {
      if (isNotFound(e)) {
        setState({ status: "gone" });
        setError(t("ask.domanda_gia_chiusa"));
      } else {
        setState({ status: "idle" });
        setError(t("ask.risposta_non_inviata", { error: errorMessage(e) }));
      }
    } finally {
      void qc.invalidateQueries({ queryKey: qk.questions() });
    }
  };

  const submit = () => {
    if (q.kind === "confirm") return;
    const built = q.kind === "form" ? formReply(q, values) : choiceReply(q, choice);
    if (!built.ok) return setError(built.error);
    void send(built.reply);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLFormElement>) => {
    if (e.nativeEvent.isComposing || e.altKey || e.ctrlKey || e.metaKey) return;
    const target = e.target as HTMLElement;
    // 1-9 pick an option while the focus is on the options, not while a field is being typed in.
    if (q.kind === "choice" && !busy && !isTypingTarget(target)) {
      const i = digitChoice(e.key, q);
      if (i !== null) {
        e.preventDefault();
        setChoice(pickChoice(q, choice, i));
        inputs.current[i]?.focus();
        return;
      }
    }
    // Enter sends, from an option or a field; on a button it does what the button does.
    if (e.key === "Enter" && !e.shiftKey && target.tagName === "INPUT") {
      e.preventDefault();
      submit();
    }
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    submit();
  };

  const count = choiceCount(q);
  return (
    <form ref={root} className="ask ask-open" aria-labelledby={titleId} aria-busy={state.status === "sending" || undefined} onSubmit={onSubmit} onKeyDown={onKeyDown} noValidate>
      <p className="sr-only" role="status">{said}</p>
      <span className="ask-eyebrow" aria-hidden="true">{t("ask.l_assistente_ha_una_domanda")}</span>
      <header className="ask-head">
        <Icon name="chat" size={16} />
        <strong id={titleId} className="ask-title">{questionTitle(q)}</strong>
      </header>
      <p id={messageId} className="ask-message">{q.message}</p>

      {q.kind === "choice" && (
        <>
          <div role={q.multiple ? "group" : "radiogroup"} aria-labelledby={messageId} aria-describedby={count > 1 ? hintId : undefined} className="ask-options">
            {q.options.map((o, i) => {
              const on = choice.selected.includes(o.id);
              const labelId = `${uid}-o${i}`;
              return (
                <label key={o.id} className={cx("ask-opt", on && "is-on")}>
                  <input
                    ref={(el) => { inputs.current[i] = el; }} type={q.multiple ? "checkbox" : "radio"} name={`${uid}-c`} value={o.id} checked={on} disabled={busy}
                    data-ask-first={i === 0 || undefined} aria-labelledby={labelId} aria-describedby={o.description ? `${labelId}-d` : undefined} aria-keyshortcuts={i < 9 ? String(i + 1) : undefined}
                    onChange={() => setChoice(pickChoice(q, choice, i))}
                  />
                  <span className="ask-opt-body">
                    <span id={labelId}>{o.label}</span>
                    {o.description && <span id={`${labelId}-d`} className="ask-opt-desc">{o.description}</span>}
                  </span>
                  {i < 9 && <kbd aria-hidden="true">{i + 1}</kbd>}
                </label>
              );
            })}
            {q.allow_other && (
              <div className={cx("ask-opt ask-opt-other", choice.otherOn && "is-on")}>
                <label className="ask-opt-pick">
                  <input
                    ref={(el) => { inputs.current[q.options.length] = el; }} type={q.multiple ? "checkbox" : "radio"} name={`${uid}-c`} checked={choice.otherOn} disabled={busy}
                    data-ask-first={q.options.length === 0 || undefined} aria-keyshortcuts={q.options.length < 9 ? String(q.options.length + 1) : undefined}
                    onChange={() => setChoice(pickChoice(q, choice, q.options.length))}
                  />
                  <span className="ask-opt-body">{t("ask.altro")}</span>
                  {q.options.length < 9 && <kbd aria-hidden="true">{q.options.length + 1}</kbd>}
                </label>
                <Input
                  type="text" aria-label={t("ask.altro_testo")} autoComplete="off" disabled={busy} value={choice.otherText}
                  onFocus={() => { if (!choice.otherOn) setChoice(pickChoice(q, choice, q.options.length)); }}
                  onChange={(e) => setChoice(typeOther(q, choice, e.target.value))}
                />
              </div>
            )}
          </div>
          {count > 1 && <p id={hintId} className="ask-hint">{t("ask.scorciatoia_cifre", { keys: count > 9 ? "1–9" : `1–${count}` })}</p>}
        </>
      )}

      {q.kind === "form" && (
        <div className="ask-form">
          {q.fields.map((f, i) => {
            const id = `${uid}-f-${f.name}`;
            const label = f.label + (f.required ? " *" : "");
            const first = i === 0 || undefined;
            if (f.type === "boolean") {
              return (
                <label key={f.name} className="ask-check">
                  <input id={id} type="checkbox" checked={values[f.name] === true} disabled={busy} data-ask-first={first} onChange={(e) => setValues({ ...values, [f.name]: e.target.checked })} />
                  <span>{f.label}</span>
                </label>
              );
            }
            return (
              <Field key={f.name} label={label} htmlFor={id}>
                {f.type === "enum" ? (
                  <Select id={id} value={String(values[f.name] ?? "")} disabled={busy} data-ask-first={first} aria-required={f.required || undefined} onChange={(e) => setValues({ ...values, [f.name]: e.target.value })}>
                    {(f.options ?? []).map((o) => <option key={o} value={o}>{o}</option>)}
                  </Select>
                ) : (
                  <Input
                    id={id} value={String(values[f.name] ?? "")} placeholder={f.placeholder ?? undefined} disabled={busy} data-ask-first={first} aria-required={f.required || undefined}
                    inputMode={f.type === "string" ? undefined : f.type === "integer" ? "numeric" : "decimal"} autoComplete="off"
                    onChange={(e) => setValues({ ...values, [f.name]: e.target.value })}
                  />
                )}
              </Field>
            );
          })}
        </div>
      )}

      {error && <p id={errorId} className="ask-error" role="alert">{error}</p>}

      <div className="ask-actions">
        {q.kind === "confirm" ? (
          <>
            <Button variant="primary" icon="check" disabled={busy} data-ask-first onClick={() => void send(confirmReply(true))}>{q.confirm_label?.trim() || t("ask.conferma")}</Button>
            <Button disabled={busy} onClick={() => void send(confirmReply(false))}>{q.deny_label?.trim() || t("ask.no")}</Button>
          </>
        ) : (
          <Button type="submit" variant="primary" icon="send" disabled={busy}>{state.status === "sending" ? t("ask.invio_in_corso") : t("ask.invia")}</Button>
        )}
        <Button variant="ghost" disabled={busy} onClick={() => void send({ action: "decline" })}>{t("ask.non_ora")}</Button>
      </div>
    </form>
  );
}
