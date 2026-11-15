import { useId, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import type { Question } from "../../shared/api/types";
import { errorMessage, isNotFound } from "../../shared/lib/errors";
import { qk } from "../../shared/query/keys";
import { Button, Icon, SkeletonLines } from "../../shared/ui";
import { confirmReply } from "../ask/model";
import { useAnnounce, useFocusOnOpen } from "../ask/useAskFocus";
import "../ask/ask.css";
import { flushBrowserInput, useBrowserState } from "./api";
import { BrowserLive } from "./BrowserLive";
import { useBrowserUi } from "./context";
import "./browser.css";

import { t } from "../../shared/i18n";

type Sending = "idle" | "sending" | "sent" | "gone";

/**
 * The request of the agent for the person to step in (log in, type a verification code in the page), as a row of the conversation: what
 * to do, the live view of the browser with its controls, «Ho finito» and «Non posso ora». The panel of the chat shows the same view; while
 * it is open the card points to it instead of showing a second one (two views would poll twice and both would take the keys).
 *
 * The focus goes to the text of the request, never to the viewer or to a button: a person who is typing in the message box when the card
 * appears must not have their keys sent to a page, or answer by accident.
 *
 * It is not a `<form>`: the field that types into the page is a password field, and a form that disappears after a request is what a
 * browser takes for a sign-in to remember (the view and its keys are outside a form in the panel too). The two buttons are `type="button"`.
 * The wording of the title and of the buttons is the interface's own, in its language: the engine's is always Italian.
 */
export function HandoverCard({ q }: { q: Question }) {
  const api = useApi();
  const qc = useQueryClient();
  const uid = useId();
  const root = useRef<HTMLDivElement>(null);
  const { panelOpen } = useBrowserUi();
  const state = useBrowserState({ poll: false }).data;
  const [sending, setSending] = useState<Sending>("idle");
  const [error, setError] = useState<string | null>(null);
  const said = useAnnounce(t("browser.handover_testo", { message: q.message }));
  useFocusOnOpen(root);
  const busy = sending !== "idle";
  const titleId = `${uid}-t`;

  /** The reply goes like any answer to a question; the row closes when the engine says so (`aiw.question_resolved`). */
  const send = async (reply: Parameters<typeof api.answerQuestion>[1]) => {
    if (busy) return;
    setError(null);
    setSending("sending");
    try {
      // The last keys typed must be in the page before the browser goes back to the agent (the engine refuses an input after that).
      await flushBrowserInput();
      await api.answerQuestion(q.id, reply);
      setSending("sent");
    } catch (e) {
      if (isNotFound(e)) {
        setSending("gone");
        setError(t("ask.domanda_gia_chiusa"));
      } else {
        setSending("idle");
        setError(t("ask.risposta_non_inviata", { error: errorMessage(e) }));
      }
    } finally {
      void qc.invalidateQueries({ queryKey: qk.questions() });
      void qc.invalidateQueries({ queryKey: qk.browser() });
      // The agent gets the browser back when its tool wakes up, a moment after the answer: look again then, not after the whole poll.
      for (const ms of [400, 1_500]) setTimeout(() => void qc.invalidateQueries({ queryKey: qk.browser() }), ms);
    }
  };

  let view;
  if (state === undefined) view = <SkeletonLines lines={3} />;
  else if (state === null) view = <p className="bl-empty" role="note">{t("browser.handover_non_disponibile")}</p>;
  else if (panelOpen) view = <p className="bl-empty" role="note">{t("browser.handover_nel_pannello")}</p>;
  else view = <BrowserLive state={state} variant="card" hideGiveBack />;

  return (
    <div ref={root} className="ask ask-open ask-handover" role="group" aria-labelledby={titleId} aria-busy={sending === "sending" || undefined}>
      <p className="sr-only" role="status">{said}</p>
      <span className="ask-eyebrow" aria-hidden="true">{t("browser.handover_eyebrow")}</span>
      <header className="ask-head">
        <Icon name="globe" size={16} />
        <strong id={titleId} className="ask-title">{t("browser.handover_titolo")}</strong>
      </header>
      <p className="ask-message" tabIndex={-1} data-ask-first>{q.message}</p>
      {view}
      {error && <p className="ask-error" role="alert">{error}</p>}
      <div className="ask-actions">
        <Button variant="primary" icon="check" disabled={busy} onClick={() => void send(confirmReply(true))}>{t("browser.handover_fatto")}</Button>
        <Button disabled={busy} onClick={() => void send({ action: "decline" })}>{t("browser.handover_non_posso")}</Button>
      </div>
    </div>
  );
}
