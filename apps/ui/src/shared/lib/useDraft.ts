import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from "react";

export interface DraftState<D> {
  draft: D;
  /** What the draft belongs to (an agent id): a different scope always starts a new draft. */
  scope: string;
  /** Serialized saved value the draft started from (or was last rebased on). */
  base: string;
}

/**
 * Pure part of `useDraft`: decides what the draft becomes when the saved value or the scope changes.
 *  - no draft yet, or another scope → a fresh draft from the saved value;
 *  - the saved value changed while the user has not edited → the draft follows it;
 *  - the saved value changed and equals the draft (the answer to our own save) → same draft, new base;
 *  - the saved value changed while the user is editing → keep the draft (their work wins).
 * `draftKey` is the serialized draft in the same form as `savedKey`.
 */
export function syncDraft<D>(state: DraftState<D> | null, input: { savedKey: string | null; scope: string; draftKey: string | null; make: () => D }): DraftState<D> | null {
  const { savedKey, scope, draftKey, make } = input;
  if (savedKey === null) return state;
  if (state === null || state.scope !== scope) return { draft: make(), scope, base: savedKey };
  if (savedKey === state.base) return state;
  if (draftKey === savedKey) return { ...state, base: savedKey };
  if (draftKey === state.base) return { draft: make(), scope, base: savedKey };
  return state;
}

/**
 * Editable copy of a value that lives on the server. `saved` is the server value, `fromSaved` builds
 * the editable form (for example rows with stable ids) and `toSaved` goes back; `normalize` makes the
 * two comparable (trimming, defaults). `dirty` is true when the draft differs from where it started.
 */
export function useDraft<S, D>({ saved, fromSaved, toSaved, normalize, scope = "" }: {
  saved: S | undefined;
  fromSaved: (s: S) => D;
  toSaved: (d: D) => S;
  normalize?: (s: S) => S;
  scope?: string;
}) {
  const [state, setState] = useState<DraftState<D> | null>(null);
  const fns = useRef({ fromSaved, norm: normalize ?? ((s: S) => s) });
  fns.current = { fromSaved, norm: normalize ?? ((s: S) => s) };
  const key = (v: S) => JSON.stringify(fns.current.norm(v));
  const savedKey = saved === undefined ? null : key(saved);
  const draftKey = state ? key(toSaved(state.draft)) : null;
  const next = syncDraft(state, { savedKey, scope, draftKey, make: () => fromSaved(saved as S) });
  // Adjusting state while rendering is the supported way to derive it from props (no extra paint).
  if (next !== state) setState(next);

  const current = next;
  const currentKey = current === state ? draftKey : current ? key(toSaved(current.draft)) : null;
  const dirty = current !== null && currentKey !== current.base;

  const setDraft: Dispatch<SetStateAction<D>> = useCallback((u) => {
    setState((s) => (s ? { ...s, draft: typeof u === "function" ? (u as (d: D) => D)(s.draft) : u } : s));
  }, []);
  const reset = useCallback(() => {
    if (saved !== undefined) setState({ draft: fns.current.fromSaved(saved), scope, base: JSON.stringify(fns.current.norm(saved)) });
  }, [saved, scope]);

  return { draft: current?.draft ?? null, setDraft, dirty, reset, stale: current !== null && savedKey !== null && savedKey !== current.base };
}
