import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { cx } from "../../shared/lib/cx";
import { errorMessage } from "../../shared/lib/errors";
import { qk } from "../../shared/query/keys";
import type { SecretRequest } from "../../shared/api/types";
import { Button, Icon } from "../../shared/ui";
import type { SecretItem } from "../chat/model/items";
import { secretClosedText, secretFailure } from "./model";
import { useAnnounce, useFocusOnOpen } from "./useAskFocus";
import "./ask.css";

import { t } from "../../shared/i18n";
/**
 * The request of the agent for a secret (a token, a password), as a row of the conversation: a protected field while it waits,
 * one line once it is closed. The value lives only in the state of this component: it goes in the body of the answer and
 * nowhere else (no draft, no storage, no query or mutation cache, no toast), and it is cleared when it is sent and when the
 * card goes away.
 */
export function SecretPrompt({ it }: { it: SecretItem }) {
  return it.outcome === undefined ? <OpenSecret req={it.req} /> : <ClosedSecret req={it.req} outcome={it.outcome} />;
}

function ClosedSecret({ req, outcome }: { req: SecretRequest; outcome: NonNullable<SecretItem["outcome"]> }) {
  return (
    <div className={cx("ask ask-closed", outcome === "stored" && "is-ok")}>
      <Icon name={outcome === "stored" ? "check" : "lock"} size={15} />
      <div className="ask-closed-text">
        <span className="ask-closed-a">{secretClosedText(req.label, outcome)}</span>
      </div>
    </div>
  );
}

type Sending = { status: "idle" } | { status: "sending" } | { status: "sent" } | { status: "forbidden" } | { status: "gone" };

function OpenSecret({ req }: { req: SecretRequest }) {
  const api = useApi();
  const qc = useQueryClient();
  const uid = useId();
  const root = useRef<HTMLFormElement>(null);
  const field = useRef<HTMLInputElement>(null);
  // The one place the value lives.
  const [value, setValue] = useState("");
  const [shown, setShown] = useState(false);
  const [state, setState] = useState<Sending>({ status: "idle" });
  const [error, setError] = useState<string | null>(null);
  const said = useAnnounce(t("ask.segreto_testo", { label: req.label }));
  useFocusOnOpen(root);
  const busy = state.status === "sending" || state.status === "sent" || state.status === "gone";

  // The card goes away: nothing of the value stays in the field either.
  useEffect(() => {
    const el = field.current;
    return () => {
      if (el) el.value = "";
    };
  }, []);

  // A refused value: the field is enabled again, with the focus in it for the next try.
  useEffect(() => {
    if (error && state.status === "idle") field.current?.focus();
  }, [error, state.status]);

  const finish = (e: unknown, sent: string): void => {
    const kind = secretFailure(e);
    if (kind === "forbidden") return setState({ status: "forbidden" });
    if (kind === "closed") {
      setState({ status: "gone" });
      return setError(t("ask.segreto_gia_chiuso"));
    }
    setState({ status: "idle" });
    // The message is the server's: it should never repeat the value, and if it did it would not get on the screen.
    const reason = sent ? errorMessage(e).split(sent).join("•••") : errorMessage(e);
    setError(kind === "rejected" ? t("ask.segreto_rifiutato", { error: reason }) : t("ask.risposta_non_inviata", { error: reason }));
  };

  const provide = async () => {
    const sent = value;
    if (sent === "" || busy) return;
    // Cleared at once, whatever the answer: a refused value is typed again, not kept to be sent again.
    setValue("");
    setShown(false);
    setError(null);
    setState({ status: "sending" });
    try {
      await api.answerSecretRequest(req.id, { action: "provide", value: sent });
      // The row closes when the engine says so (`aiw.secret_resolved`).
      setState({ status: "sent" });
    } catch (e) {
      finish(e, sent);
    } finally {
      void qc.invalidateQueries({ queryKey: qk.secretRequests() });
    }
  };

  const decline = async () => {
    if (busy) return;
    setValue("");
    setError(null);
    setState({ status: "sending" });
    try {
      await api.answerSecretRequest(req.id, { action: "decline" });
      setState({ status: "sent" });
    } catch (e) {
      finish(e, "");
    } finally {
      void qc.invalidateQueries({ queryKey: qk.secretRequests() });
    }
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    void provide();
  };

  const titleId = `${uid}-t`;
  const fieldId = `${uid}-v`;
  const noteId = `${uid}-n`;
  const forbidden = state.status === "forbidden";
  return (
    <form ref={root} className="ask ask-open ask-secret" aria-labelledby={titleId} aria-busy={state.status === "sending" || undefined} onSubmit={onSubmit} noValidate>
      <p className="sr-only" role="status">{said}</p>
      <span className="ask-eyebrow" aria-hidden="true">{t("ask.segreto_richiesto")}</span>
      <header className="ask-head">
        <Icon name="lock" size={16} />
        <strong id={titleId} className="ask-title">{req.label}</strong>
      </header>
      {req.reason && <p className="ask-reason">{req.reason}</p>}
      {req.destination && <p className="ask-dest">{t("ask.segreto_destinazione", { destination: req.destination })}</p>}

      {forbidden ? (
        <p className="ask-message" role="alert"><b>{t("ask.segreto_dall_app_titolo")}</b> {t("ask.segreto_dall_app")}</p>
      ) : (
        <>
          <div className="ask-secret-field">
            <label htmlFor={fieldId}>{t("ask.segreto_campo", { label: req.label })}</label>
            <div className="ask-secret-row">
              <input
                ref={field} id={fieldId} className="input mono" type={shown ? "text" : "password"} value={value} disabled={busy} data-ask-first
                autoComplete="off" autoCapitalize="off" autoCorrect="off" spellCheck={false} aria-describedby={noteId}
                data-1p-ignore="true" data-lpignore="true" data-bwignore="true" data-form-type="other"
                onChange={(e) => setValue(e.target.value)}
              />
              <Button size="sm" variant="ghost" aria-pressed={shown} disabled={busy} onClick={() => setShown((v) => !v)}>{t("ask.mostra_il_valore")}</Button>
            </div>
          </div>
          <p id={noteId} className="ask-hint-private"><Icon name="shield" size={14} /><span>{t("ask.segreto_riservato")}</span></p>
        </>
      )}

      {error && <p className="ask-error" role="alert">{error}</p>}

      {!forbidden && (
        <div className="ask-actions">
          <Button type="submit" variant="primary" icon="lock" disabled={busy || value === ""}>{state.status === "sending" ? t("ask.salvataggio_in_corso") : t("ask.salva")}</Button>
          <Button variant="ghost" disabled={busy} onClick={() => void decline()}>{t("ask.non_ora")}</Button>
        </div>
      )}
    </form>
  );
}
