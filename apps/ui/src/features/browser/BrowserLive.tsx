// Live view of the controlled browser and the human takeover (issue #73). Limits, in short: the page is an image; a screen reader gets the state,
// the title and the host of the page, the control that has the focus in it (never its value) and the commands, not a list of its elements.
import { useCallback, useEffect, useId, useRef, useState, type ClipboardEvent, type KeyboardEvent, type MouseEvent, type PointerEvent } from "react";
import type { BrowserDialog, BrowserState } from "../../shared/api/types";
import { cx } from "../../shared/lib/cx";
import { Button, Icon } from "../../shared/ui";
import { useAnnounce } from "../ask/useAskFocus";
import { onFlushBrowserInput, useBrowserActions, useBrowserFrames, useDocumentHidden, type FrameView } from "./api";
import { clickInput, clipDialogMessage, connectionText, controlsFor, dialogShape, fractionIn, frameAlt, frameDelay, keyAction, keyInput, notes, pageLine, scrollInput, splitText, statusPhrase, wheelPixels, type ButtonKey } from "./model";
import "./browser.css";

import { t } from "../../shared/i18n";

/**
 * The live view of the controlled browser: the page as an image, who drives it, and the controls to pause the agent, take the browser
 * (log in, type a verification code) and give it back. It is the same component in the side panel and in the card of a request for help.
 *
 * Limits: the page is an image. A screen reader gets the state, the title and the host, the control that has the focus in the page
 * (`state.focus`, without its value) and the commands; it does not get a list of the elements of the page. A sighted person who can
 * not use a pointer can click only with the keyboard through the buttons (Tab, Enter, Backspace) and the field that types into the
 * page; clicking on a spot of the page needs a pointer or a touch.
 *
 * Nothing of what passes here is kept: a frame lives in the state of this component, the keys and the text go to the engine and are
 * forgotten (no log, no storage, no query or mutation cache), and the polling stops when the component goes away. The fields the person
 * types in are not controlled by React: what is typed is in the field and nowhere else (not in the `value` attribute of the markup, not in a
 * state), and the field is emptied when it is sent.
 */
export function BrowserLive({ state, variant, hideGiveBack = false }: { state: BrowserState; variant: "panel" | "card"; hideGiveBack?: boolean }) {
  const uid = useId();
  const hidden = useDocumentHidden();
  const actions = useBrowserActions();
  const { refreshState, gone } = actions;
  const delay = frameDelay({ running: state.running, mode: state.mode, agentBusy: state.agent_busy, hidden, dialog: state.dialog !== null });
  const frames = useBrowserFrames(delay, { running: state.running, onBlocked: refreshState, onGone: gone });
  const { refresh } = frames;
  const c = controlsFor(state);

  // After the person did something: look at the page and at the state (the control with the focus) again soon, but not at every key.
  const soon = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const afterInput = useCallback(() => {
    if (soon.current !== undefined) return;
    soon.current = setTimeout(() => {
      soon.current = undefined;
      refresh();
      refreshState();
    }, 250);
  }, [refresh, refreshState]);
  useEffect(() => () => {
    if (soon.current !== undefined) clearTimeout(soon.current);
  }, []);

  const press = (key: ButtonKey, shift = false) => {
    actions.send(keyInput(key, shift));
    afterInput();
  };

  if (!state.running) {
    return (
      <div className={cx("bl", `bl-${variant}`)}>
        <p className="bl-status" role="status">{statusPhrase(state)}</p>
        <p className="bl-empty">{t("browser.non_aperto_aiuto")}</p>
      </div>
    );
  }

  const human = state.mode === "human";
  const hintId = `${uid}-h`;
  const reason = variant === "panel" ? state.handover?.reason : undefined;
  return (
    <div className={cx("bl", `bl-${variant}`)} data-mode={state.mode} aria-busy={actions.pending !== null || undefined}>
      <div className="bl-head">
        <p className={cx("bl-status", `is-${state.mode}`)} role="status">{statusPhrase(state)}</p>
        <p className="bl-page">
          <span className={cx("bl-conn", state.page?.secure ? "is-secure" : "is-insecure")}>
            <Icon name={state.page?.secure ? "lock" : "alert"} size={13} />
            <span className="sr-only">{state.page ? connectionText(state.page.secure) : ""}</span>
            {state.page && <span aria-hidden="true">{state.page.secure ? "https" : "http"}</span>}
          </span>
          <span className="bl-page-text">{pageLine(state.page)}</span>
        </p>
      </div>

      {reason && (
        <p className="bl-reason"><b>{t("browser.richiesta_agente")}</b> {reason}</p>
      )}

      <div className="bl-actions" role="group" aria-label={t("browser.comandi")}>
        <Button
          icon={c.canResume ? "play" : "pause"} disabled={!(c.canPause || c.canResume)}
          title={c.canResume ? t("browser.riprendi_hint") : t("browser.pausa_hint")}
          onClick={() => { if (actions.pending === null) void actions.control(c.canResume ? "resume" : "pause"); }}
        >
          {c.canResume ? t("browser.riprendi") : t("browser.pausa")}
        </Button>
        {!(human && hideGiveBack) && (
          <Button
            icon={c.canGiveBack ? "agent" : "mouse"} variant={c.canGiveBack ? "primary" : "default"}
            title={c.canGiveBack ? t("browser.restituisci_hint") : t("browser.prendi_hint")}
            onClick={() => { if (actions.pending === null) void actions.control(c.canGiveBack ? "give_back" : "take"); }}
          >
            {c.canGiveBack ? t("browser.restituisci") : t("browser.prendi")}
          </Button>
        )}
      </div>

      {actions.problem && <p className="bl-problem" role="alert">{actions.problem}</p>}

      {state.dialog && (
        <SiteDialog
          dialog={state.dialog} canAnswer={c.canAnswerDialog && !actions.answering}
          onAnswer={(accept, text) => void actions.answerDialog({ action: accept ? "accept" : "dismiss", ...(text !== undefined ? { text } : {}) })}
        />
      )}

      <Viewer frame={frames.frame} state={state} interactive={c.canType} hintId={hintId} send={actions.send} afterInput={afterInput} />
      {frames.error && <p className="bl-problem">{t("browser.vista_errore", { error: frames.error })}</p>}
      {human && <p id={hintId} className="bl-hint">{t("browser.visore_istruzioni")}</p>}

      {human && <TypeBar disabled={!c.canType} press={press} send={actions.send} afterInput={afterInput} />}
      {human && (
        <p className="bl-focus" role="status" aria-live="polite">{state.focus ? t("browser.focus_pagina", { focus: state.focus }) : ""}</p>
      )}

      <ul className="bl-notes">
        {notes(state).map((n) => <li key={n.id} data-note={n.id}>{n.text}</li>)}
      </ul>
    </div>
  );
}

// ---------------------------------------------------------------- the page

/** A touch moves the page like a finger on a phone: only past this many pixels is it a scroll, not a tap. */
const TOUCH_SLOP = 6;
/** The wheel and the finger are gathered for this long: one request with the sum, not one per event. */
const SCROLL_BATCH_MS = 100;

function Viewer({ frame, state, interactive, hintId, send, afterInput }: {
  frame: FrameView | null;
  state: BrowserState;
  interactive: boolean;
  hintId: string;
  send: ReturnType<typeof useBrowserActions>["send"];
  afterInput: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const img = useRef<HTMLImageElement>(null);
  const touch = useRef<{ y: number; moved: boolean } | null>(null);
  const swallowClick = useRef(false);
  const alt = frameAlt(state.page);
  const height = frame?.height ?? 600;

  // Wheel and finger are gathered into one scroll request every moment: the sum of the distances, at the last place they were.
  const gathered = useRef<{ sum: number; x: number; y: number; timer: ReturnType<typeof setTimeout> | undefined }>({ sum: 0, x: 0.5, y: 0.5, timer: undefined });
  const flushScroll = useCallback(() => {
    const g = gathered.current;
    if (g.timer !== undefined) clearTimeout(g.timer);
    g.timer = undefined;
    const dy = Math.round(g.sum);
    g.sum = 0;
    const rect = img.current?.getBoundingClientRect();
    const input = rect ? scrollInput(rect, rect.left + g.x * rect.width, rect.top + g.y * rect.height, dy) : null;
    if (input) {
      send(input);
      afterInput();
    }
  }, [send, afterInput]);
  const pushScroll = useCallback(
    (dy: number, x: number, y: number) => {
      const g = gathered.current;
      g.sum += dy;
      g.x = x;
      g.y = y;
      g.timer ??= setTimeout(flushScroll, SCROLL_BATCH_MS);
    },
    [flushScroll],
  );
  // What is still gathered goes out before the browser goes back to the agent, and is dropped with the view.
  useEffect(() => {
    const off = onFlushBrowserInput(flushScroll);
    const g = gathered.current;
    return () => {
      off();
      if (g.timer !== undefined) clearTimeout(g.timer);
      g.timer = undefined;
    };
  }, [flushScroll]);

  // The wheel needs a listener that may cancel it (React's is passive): the page scrolls, not the panel.
  useEffect(() => {
    const el = box.current;
    if (!el || !interactive) return;
    const onWheel = (e: globalThis.WheelEvent) => {
      if (e.ctrlKey || e.metaKey) return; // a pinch or the browser's own zoom
      e.preventDefault();
      const rect = img.current?.getBoundingClientRect();
      const at = rect ? fractionIn(rect, e.clientX, e.clientY) : null;
      if (at) pushScroll(wheelPixels(e.deltaY, e.deltaMode, height), at.x, at.y);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [interactive, pushScroll, height]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const a = keyAction(e.nativeEvent);
    if (a.action !== "send") return;
    // The key is the page's: neither the panel, nor a shortcut of the app, nor the browser sees it.
    e.preventDefault();
    e.stopPropagation();
    send(a.input);
    afterInput();
  };
  const onPaste = (e: ClipboardEvent<HTMLDivElement>) => {
    const pasted = e.clipboardData.getData("text/plain");
    if (!pasted) return;
    e.preventDefault();
    for (const part of splitText(pasted)) send({ type: "text", text: part });
    afterInput();
  };
  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    if (swallowClick.current) {
      swallowClick.current = false;
      return;
    }
    const rect = img.current?.getBoundingClientRect();
    const input = rect ? clickInput(rect, e) : null;
    if (!input) return;
    send(input);
    afterInput();
  };
  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.pointerType === "touch") touch.current = { y: e.clientY, moved: false };
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const t0 = touch.current;
    if (!t0 || e.pointerType !== "touch") return;
    const dy = t0.y - e.clientY;
    if (!t0.moved && Math.abs(dy) < TOUCH_SLOP) return;
    const rect = img.current?.getBoundingClientRect();
    const at = rect ? fractionIn(rect, e.clientX, e.clientY) : null;
    if (!rect || !frame || !at) return;
    t0.moved = true;
    t0.y = e.clientY;
    // A finger moves the page by the same distance on the screen, which is `frame.width / rect.width` pixels of the page per pixel.
    pushScroll(dy * (frame.width / rect.width), at.x, at.y);
  };
  const onPointerEnd = () => {
    if (touch.current?.moved) {
      swallowClick.current = true;
      flushScroll();
    }
    touch.current = null;
  };

  return (
    <div
      ref={box} className={cx("bl-viewer", interactive && "is-live", !frame && "is-empty")}
      {...(interactive
        ? {
            role: "application", tabIndex: 0, "data-own-keys": "", "aria-label": t("browser.visore_nome", { page: pageLine(state.page) }), "aria-describedby": hintId,
            onKeyDown, onPaste, onClick, onPointerDown, onPointerMove, onPointerUp: onPointerEnd, onPointerCancel: onPointerEnd,
          }
        : {})}
    >
      {frame ? (
        <img ref={img} src={frame.image} width={frame.width} height={frame.height} alt={alt} draggable={false} />
      ) : (
        <p className="bl-loading">{t("browser.carico")}</p>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- the keyboard of the phone and of the password manager

function TypeBar({ disabled, press, send, afterInput }: {
  disabled: boolean;
  press: (key: ButtonKey, shift?: boolean) => void;
  send: ReturnType<typeof useBrowserActions>["send"];
  afterInput: () => void;
}) {
  const uid = useId();
  // The field is not controlled: the text is in the field and nowhere else (not in a state, not in the `value` attribute of the markup); only
  // whether there is some is known here, for «Invia». It goes to the page when sent and the field is emptied at once.
  const [filled, setFilled] = useState(false);
  const [shown, setShown] = useState(false);
  const field = useRef<HTMLInputElement>(null);

  // The card goes away: nothing of what was typed stays in the field either.
  useEffect(() => {
    const el = field.current;
    return () => {
      if (el) el.value = "";
    };
  }, []);

  const submit = () => {
    const el = field.current;
    if (disabled || !el || el.value === "") return;
    const parts = splitText(el.value);
    el.value = "";
    setFilled(false);
    for (const part of parts) send({ type: "text", text: part });
    afterInput();
    el.focus();
  };

  return (
    <div className="bl-keys" role="group" aria-label={t("browser.tastiera")}>
      <div className="bl-keybtns">
        <Button size="sm" disabled={disabled} title={t("browser.tasto_tab_hint")} onClick={() => press("Tab")}>{t("browser.tasto_tab")}</Button>
        <Button size="sm" disabled={disabled} title={t("browser.tasto_maiusc_tab_hint")} onClick={() => press("Tab", true)}>{t("browser.tasto_maiusc_tab")}</Button>
        <Button size="sm" disabled={disabled} title={t("browser.tasto_invio_hint")} onClick={() => press("Enter")}>{t("browser.tasto_invio")}</Button>
        <Button size="sm" disabled={disabled} title={t("browser.tasto_cancella_hint")} onClick={() => press("Backspace")}>{t("browser.tasto_cancella")}</Button>
      </div>
      <div className="bl-type">
        <label htmlFor={`${uid}-t`}>{t("browser.scrivi_etichetta")}</label>
        <div className="bl-type-row">
          <input
            ref={field} id={`${uid}-t`} className="input" type={shown ? "text" : "password"} disabled={disabled} autoComplete="off" autoCorrect="off" autoCapitalize="off" spellCheck={false}
            data-1p-ignore="true" data-lpignore="true" data-bwignore="true" data-form-type="other" aria-describedby={`${uid}-p`}
            onInput={(e) => setFilled(e.currentTarget.value !== "")}
            onKeyDown={(e) => {
              if (e.key !== "Enter" || e.nativeEvent.isComposing) return;
              e.preventDefault();
              e.stopPropagation();
              submit();
            }}
          />
          <Button size="sm" disabled={disabled} onClick={() => setShown(!shown)}>{shown ? t("browser.nascondi_testo") : t("browser.mostra_testo")}</Button>
          <Button variant="primary" icon="send" disabled={disabled || !filled} onClick={submit}>{t("browser.invia")}</Button>
        </div>
        <p id={`${uid}-p`} className="bl-privacy"><Icon name="lock" size={13} /> <span>{t("browser.privacy")}</span></p>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- a window of the site

/** Is the focus where the view is (or nowhere)? Only then does a window of the site take it: a person typing elsewhere keeps their place. */
export function focusIsInView(active: Element | null): boolean {
  if (!active || active === document.body || active === document.documentElement) return true;
  return active.closest(".bl, .ws-browser, .bl-sheet, .bl-drawer") !== null;
}

function SiteDialog({ dialog, canAnswer, onAnswer }: { dialog: BrowserDialog; canAnswer: boolean; onAnswer: (accept: boolean, text?: string) => void }) {
  const uid = useId();
  const shape = dialogShape(dialog);
  const root = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const said = useAnnounce(t("browser.finestra_annuncio", { title: shape.title, message: clipDialogMessage(dialog.message) }));
  // A window of the site stops the page: the person is taken to its answer (the field of a prompt, else «OK»), but only from the view or from
  // nowhere. From anywhere else (the message box, say) the focus stays and the live region above says that a window waits.
  useEffect(() => {
    if (canAnswer && focusIsInView(document.activeElement)) root.current?.querySelector<HTMLElement>("input, button")?.focus({ preventScroll: true });
  }, [canAnswer, dialog.kind, dialog.message]);

  const accept = () => onAnswer(true, shape.field ? field.current?.value ?? dialog.default_text : undefined);
  return (
    <div
      ref={root} className="bl-dialog" role="alertdialog" data-own-keys="" aria-labelledby={`${uid}-t`} aria-describedby={`${uid}-m`}
      onKeyDown={(e) => {
        if (!canAnswer || e.nativeEvent.isComposing) return;
        if (e.key === "Escape" && shape.cancel) {
          e.preventDefault();
          e.stopPropagation();
          onAnswer(false);
        }
      }}
    >
      <p className="sr-only" role="status">{said}</p>
      <strong id={`${uid}-t`} className="bl-dialog-title">{shape.title}</strong>
      <p id={`${uid}-m`} className="bl-dialog-msg">{clipDialogMessage(dialog.message)}</p>
      {shape.field && (
        // Not controlled, and a new window is a new field: what is typed is in the field and nowhere else.
        <input
          key={`${dialog.kind}|${dialog.message}|${dialog.default_text}`} ref={field} className="input" aria-label={t("browser.finestra_campo")}
          defaultValue={dialog.default_text} disabled={!canAnswer} autoComplete="off" spellCheck={false}
          onKeyDown={(e) => {
            if (e.key !== "Enter" || e.nativeEvent.isComposing) return;
            e.preventDefault();
            if (canAnswer) accept();
          }}
        />
      )}
      <div className="bl-dialog-actions">
        <Button variant="primary" icon="check" disabled={!canAnswer} onClick={accept}>{t("browser.finestra_ok")}</Button>
        {shape.cancel && <Button disabled={!canAnswer} onClick={() => onAnswer(false)}>{t("browser.finestra_annulla")}</Button>}
      </div>
    </div>
  );
}
