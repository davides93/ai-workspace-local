// A gate in front of the hash router: a form with unsaved changes can veto a navigation *before* the
// new route renders (a route that renders first resets the form it is leaving).
//
// The router calls `check()` on every `hashchange`, ahead of its own re-render. A blocker receives the
// hash being reached and the one currently rendered; returning true vetoes the change, and the blocker
// is responsible for putting the old hash back (`history.replaceState`).

export type Blocker = (to: string, from: string) => boolean;

export function createNavGate(read: () => string) {
  const blockers = new Set<Blocker>();
  /** The hash the router last rendered. */
  let rendered = read();
  return {
    /** Registers a blocker; returns the function that removes it. */
    block(fn: Blocker): () => void {
      blockers.add(fn);
      return () => void blockers.delete(fn);
    },
    /** True when the router may render the current hash; false when a blocker kept it where it was. */
    check(): boolean {
      const to = read();
      if (to === rendered) return true;
      for (const b of [...blockers]) if (b(to, rendered)) return false;
      rendered = to;
      return true;
    },
  };
}

export const navGate = createNavGate(() => (typeof location === "undefined" ? "" : location.hash));

/** Vetoes navigations while a form has unsaved changes (see `useUnsavedGuard`). */
export const blockNavigation = navGate.block;
