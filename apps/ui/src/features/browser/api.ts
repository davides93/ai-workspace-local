import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import type { BrowserControlAction, BrowserDialogReply, BrowserInput, BrowserState } from "../../shared/api/types";
import { errorMessage, errorStatus } from "../../shared/lib/errors";
import { qk } from "../../shared/query/keys";
import { hidesAfterAction, hidesBrowser, normalizeState, statePollInterval } from "./model";

import { t } from "../../shared/i18n";

/**
 * The state of the controlled browser (who drives it, the page in front, a window of the site, an open request for help).
 * `null`: it is not for this device (403) or this backend does not have it (404): everything about the browser stays hidden and the
 * poll stops. `undefined`: not known yet. Poll only from one place (`poll`): the cards read the same entry.
 */
export function useBrowserState({ poll = true }: { poll?: boolean } = {}) {
  const api = useApi();
  return useQuery({
    queryKey: qk.browser(),
    queryFn: async (): Promise<BrowserState | null> => {
      try {
        return normalizeState(await api.browserState());
      } catch (e) {
        if (hidesBrowser(e)) return null;
        throw e;
      }
    },
    // Not while the tab is in the background (the default of the app), and not once the browser is known to be unavailable.
    refetchInterval: poll ? (q) => statePollInterval(q.state.data) : false,
    // Nor when the window gets the focus again: a device that may not see the browser still may not.
    refetchOnWindowFocus: (q) => q.state.data !== null,
  });
}

/** Is the page in front of the person? A hidden tab does not need frames. */
export function useDocumentHidden(): boolean {
  return useSyncExternalStore(
    (onChange) => {
      document.addEventListener("visibilitychange", onChange);
      return () => document.removeEventListener("visibilitychange", onChange);
    },
    () => document.visibilityState === "hidden",
    () => false,
  );
}

export interface FrameView {
  image: string;
  width: number;
  height: number;
}

/** A frame the screen can show: only an image data URL is put in `src`, and the size is a real one (the page's CSS pixels). */
export function toFrame(f: { image: string; width: number; height: number }): FrameView | null {
  const size = (n: number, fallback: number) => (Number.isFinite(n) && n > 0 ? Math.round(n) : fallback);
  return typeof f.image === "string" && f.image.startsWith("data:image/") ? { image: f.image, width: size(f.width, 1000), height: size(f.height, 500) } : null;
}

/**
 * The frames of the page, one after the other (never two requests at once), for as long as `delayMs` is a number (`null`: tab hidden or no browser). A frame is state of the
 * component that shows it: nothing is cached, stored or kept once the panel is gone. A 409 (a window of the site waits) keeps the last
 * frame (and the state is asked once, then the caller stops the loop with `delayMs: null` until the window is gone); a 403 or 404 ends the
 * loop and says so through `onGone`.
 */
export function useBrowserFrames(delayMs: number | null, { running, onBlocked, onGone }: { running: boolean; onBlocked: () => void; onGone: () => void }) {
  const api = useApi();
  const [frame, setFrame] = useState<FrameView | null>(null);
  const [blocked, setBlocked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const delay = useRef(delayMs);
  delay.current = delayMs;
  const callbacks = useRef({ onBlocked, onGone });
  callbacks.current = { onBlocked, onGone };
  const nudge = useRef<() => void>(() => {});
  const wasBlocked = useRef(false);
  const active = delayMs !== null;

  useEffect(() => {
    if (!active) {
      nudge.current = () => {};
      return;
    }
    let stopped = false;
    let inflight = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const run = async () => {
      timer = undefined;
      inflight = true;
      let next = delay.current ?? 1_000;
      try {
        const f = await api.browserFrame();
        if (stopped) return;
        setFrame(f.running ? toFrame(f) : null);
        wasBlocked.current = false;
        setBlocked(false);
        setError(null);
      } catch (e) {
        if (stopped) return;
        if (errorStatus(e) === 409) {
          // A window of the site waits: ask the state once (it says so, and the loop stops); never at every turn.
          if (!wasBlocked.current) callbacks.current.onBlocked();
          wasBlocked.current = true;
          setBlocked(true);
          next = Math.max(next, 1_500);
        } else if (hidesBrowser(e)) {
          callbacks.current.onGone();
          return;
        } else {
          setError(errorMessage(e));
          next = Math.max(next, 3_000);
        }
      } finally {
        inflight = false;
      }
      if (!stopped) timer = setTimeout(() => void run(), next);
    };
    // Something the person did changed the page: look again soon, not after the whole delay.
    nudge.current = () => {
      if (stopped || inflight) return;
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(() => void run(), 150);
    };
    void run();
    return () => {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [active, api]);

  // No browser, no frame: nothing of the last page stays on the screen. (A hidden tab only stops asking: it keeps the last frame.)
  useEffect(() => {
    if (!running) {
      setFrame(null);
      setBlocked(false);
    }
  }, [running]);

  return { frame: running ? frame : null, blocked, error, refresh: useCallback(() => nudge.current(), []) };
}

/**
 * Every input of the person goes through one serial queue, in the order of the keys, for the whole app (there is one browser, and one view at a
 * time takes the keys). `flushBrowserInput` is what «Restituisci» and «Ho finito» wait for, so that the last keys are in the page before it goes
 * back to the agent (the engine answers 409 to an input after that).
 */
let inputQueue: Promise<void> = Promise.resolve();
const flushers = new Set<() => void>();

/** Queues one request; a failure is the task's own business (the queue never rejects). */
export function enqueueBrowserInput(task: () => Promise<void>): void {
  inputQueue = inputQueue.then(task, task);
}

/** What waits to be gathered (a scroll) is queued now, then resolves when everything queued has been sent. */
export function flushBrowserInput(): Promise<void> {
  flushers.forEach((f) => f());
  return inputQueue;
}

/** A view registers what it gathers before sending (see `flushBrowserInput`); returns what takes it back. */
export function onFlushBrowserInput(flush: () => void): () => void {
  flushers.add(flush);
  return () => {
    flushers.delete(flush);
  };
}

/** How long an error of an input stays on the screen (it is an alert: it would be read again at every visit otherwise). */
export const PROBLEM_MS = 6_000;

/**
 * The controls of the person. Control actions use the state the engine answers with. Input goes one request at a time, in the order of the
 * keys, and nothing of it is kept: no mutation (its variables would stay in the cache), no log, no toast; a refusal is the engine's own
 * sentence, shown by the caller. Only a 403 hides the browser after an action; a 404 or 409 shows its message and asks the state again.
 */
export function useBrowserActions() {
  const api = useApi();
  const qc = useQueryClient();
  const [pending, setPending] = useState<BrowserControlAction | null>(null);
  const [answering, setAnswering] = useState(false);
  const answeringNow = useRef(false);
  const [problem, setProblem] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // An error is read once: it goes after a few seconds (and at the next input that works).
  useEffect(() => {
    if (!problem) return;
    const id = setTimeout(() => setProblem(null), PROBLEM_MS);
    return () => clearTimeout(id);
  }, [problem]);

  /** A 403 after an action: the browser is not for this device any more, everything about it goes away. */
  const gone = useCallback(() => qc.setQueryData(qk.browser(), null), [qc]);
  /** The state again, now: what the person sees must not be older than what they just did. */
  const refreshState = useCallback(() => void qc.invalidateQueries({ queryKey: qk.browser() }), [qc]);

  const control = useCallback(
    async (action: BrowserControlAction) => {
      setPending(action);
      setProblem(null);
      try {
        // The keys typed a moment ago must be in the page before it goes back to the agent.
        if (action === "give_back") await flushBrowserInput();
        const next = normalizeState(await api.browserControl(action));
        if (next) qc.setQueryData(qk.browser(), next);
      } catch (e) {
        if (hidesAfterAction(e)) gone();
        else if (alive.current) setProblem(t("browser.azione_fallita", { error: errorMessage(e) }));
      } finally {
        if (alive.current) setPending(null);
        refreshState();
      }
    },
    [api, qc, gone, refreshState],
  );

  const send = useCallback(
    (input: BrowserInput) => {
      enqueueBrowserInput(async () => {
        try {
          await api.browserInput(input);
          if (alive.current) setProblem(null);
        } catch (e) {
          if (hidesAfterAction(e)) gone();
          else {
            if (alive.current) setProblem(errorMessage(e));
            refreshState();
          }
        }
      });
    },
    [api, gone, refreshState],
  );

  /** One answer at a time: the window stays answerable only after the state says what became of it (no second answer to the same window). */
  const answerDialog = useCallback(
    async (reply: BrowserDialogReply) => {
      if (answeringNow.current) return;
      answeringNow.current = true;
      setAnswering(true);
      setProblem(null);
      try {
        await api.browserDialog(reply);
      } catch (e) {
        if (hidesAfterAction(e)) gone();
        else if (alive.current) setProblem(errorMessage(e));
      } finally {
        await qc.invalidateQueries({ queryKey: qk.browser() }).catch(() => {});
        answeringNow.current = false;
        if (alive.current) setAnswering(false);
      }
    },
    [api, qc, gone],
  );

  return { pending, answering, problem, control, send, answerDialog, refreshState, gone };
}
