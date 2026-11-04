import { useCallback, useEffect, useRef, useState } from "react";
import { blockNavigation } from "./navGate";

/** Normalizes `/agents/x`, `agents/x` and `#/agents/x` to a hash. */
function toHash(to: string): string {
  return to.startsWith("#") ? to : `#${to.startsWith("/") ? to : `/${to}`}`;
}

export interface UnsavedGuard {
  /** Hash the user tried to reach while the form had unsaved changes; null when nothing is waiting. */
  pending: string | null;
  /** Dismisses the prompt and keeps editing. */
  stay: () => void;
  /** Throws the changes away (`onDiscard`) and goes where the user wanted to go. */
  leave: () => void;
  /** Goes to `to` without asking (for navigation the form itself triggers, e.g. after a successful save). */
  go: (to: string, opts?: { replace?: boolean }) => void;
}

/**
 * Guards a form with unsaved changes against leaving it.
 *
 * The router is a hash router, so every way out (a link, the rail, the back button, a tab) ends in a
 * `hashchange`. The navigation gate (see `navGate`) asks this guard before the router renders the new
 * route: while `dirty` is true the old URL is put back and the target is kept as `pending`; the view
 * shows `UnsavedChangesDialog` and the user decides. Closing or reloading the tab asks through the
 * browser's own prompt.
 */
export function useUnsavedGuard(dirty: boolean, opts: {
  onDiscard?: () => void;
  /** True when moving between these two hashes keeps the form alive (a tab or a drawer of the same page). */
  keep?: (from: string, to: string) => boolean;
} = {}): UnsavedGuard {
  const [pending, setPending] = useState<string | null>(null);
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const discardRef = useRef(opts.onDiscard);
  discardRef.current = opts.onDiscard;
  const keepRef = useRef(opts.keep);
  keepRef.current = opts.keep;
  /** Location the form itself navigates to: let it through once. */
  const allowed = useRef<string | null>(null);

  useEffect(
    () =>
      blockNavigation((to, from) => {
        if (allowed.current === to) {
          allowed.current = null;
          return false;
        }
        if (!dirtyRef.current || keepRef.current?.(from, to)) return false;
        history.replaceState(null, "", `${location.pathname}${location.search}${from}`);
        setPending(to);
        return true;
      }),
    [],
  );

  useEffect(() => {
    if (!dirty) return;
    const onUnload = (e: BeforeUnloadEvent) => {
      e.preventDefault();
      e.returnValue = "";
    };
    window.addEventListener("beforeunload", onUnload);
    return () => window.removeEventListener("beforeunload", onUnload);
  }, [dirty]);

  const go = useCallback((to: string, o: { replace?: boolean } = {}) => {
    const hash = toHash(to);
    setPending(null);
    if (location.hash === hash) return;
    allowed.current = hash;
    if (o.replace) {
      history.replaceState(null, "", `${location.pathname}${location.search}${hash}`);
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    } else {
      location.hash = hash;
    }
  }, []);

  const stay = useCallback(() => setPending(null), []);
  const leave = useCallback(() => {
    if (pending === null) return;
    discardRef.current?.();
    go(pending);
  }, [pending, go]);

  return { pending, stay, leave, go };
}
