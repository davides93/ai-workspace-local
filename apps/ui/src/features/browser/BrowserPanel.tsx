import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { IconButton, ModalShell, SkeletonLines } from "../../shared/ui";
import type { InspectorMode } from "../chat/useLayout";
import { useBrowserState } from "./api";
import { BrowserLive } from "./BrowserLive";
import "./browser.css";

import { t } from "../../shared/i18n";

/**
 * The panel of the live view, opened from the «Browser» button of the chat header: a column next to the conversation on wide screens, a
 * side drawer on tablets, a full-screen sheet on phones. The focus goes to its title when it opens and the chat gives it back to the button
 * when it closes; Esc closes it, except while the viewer has the focus (then Esc is the page's: Tab leaves the viewer).
 */
export function BrowserPanel({ mode, onClose }: { mode: InspectorMode; onClose: () => void }) {
  const uid = useId();
  const state = useBrowserState({ poll: false }).data;
  const titleId = `${uid}-t`;
  // The browser is not for this device after all (403, 404): nothing is left to show.
  useEffect(() => {
    if (state === null) onClose();
  }, [state, onClose]);

  const body: ReactNode = state ? <BrowserLive state={state} variant="panel" /> : <SkeletonLines lines={4} />;
  const title = t("browser.titolo");
  const head = (
    <div className="overlay-head">
      <h2 id={titleId} className="grow" tabIndex={-1} data-autofocus>{title}</h2>
      <IconButton aria-label={t("browser.chiudi")} icon="close" onClick={onClose} />
    </div>
  );

  if (mode === "inline") return <InlineAside titleId={titleId} head={head} body={body} onClose={onClose} />;
  return (
    <ModalShell variant={mode === "sheet" ? "sheet" : "drawer"} title={title} className={mode === "sheet" ? "bl-sheet" : "bl-drawer"} onClose={onClose}>
      {mode === "sheet" && <div className="sheet-grip" aria-hidden="true" />}
      {head}
      <div className="overlay-body">{body}</div>
    </ModalShell>
  );
}

function InlineAside({ titleId, head, body, onClose }: { titleId: string; head: ReactNode; body: ReactNode; onClose: () => void }) {
  const root = useRef<HTMLElement>(null);
  // The focus moves into the panel when it opens (no dialog does it here).
  useEffect(() => {
    root.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus({ preventScroll: true });
  }, []);
  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key !== "Escape" || e.defaultPrevented) return;
    // The viewer sends Esc to the page, and so does a window of the site; anything else here closes the panel.
    if ((e.target as Element).closest("[data-own-keys]")) return;
    e.preventDefault();
    onClose();
  };
  return (
    <aside ref={root} className="ws-browser" aria-labelledby={titleId} onKeyDown={onKeyDown}>
      {head}
      <div className="ws-browser-body">{body}</div>
    </aside>
  );
}
