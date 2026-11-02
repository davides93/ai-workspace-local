import { useCallback, useRef, useState } from "react";
import { errorMessage } from "./errors";
import { toast } from "./toast";

interface ActionOptions<A extends unknown[], R> {
  /** Toast shown when the action succeeds. */
  success?: string | ((result: R, ...args: A) => string);
  /** Prefix for the error toast ("Impossibile avviare il run: …"). */
  errorPrefix?: string;
}

/**
 * Runs an async action with a pending flag and one shared `catch → toast`.
 * Returns `undefined` when the action failed (the error was already shown).
 */
export function useAction<A extends unknown[], R>(fn: (...args: A) => Promise<R>, opts: ActionOptions<A, R> = {}) {
  const [pending, setPending] = useState(false);
  const fnRef = useRef(fn);
  fnRef.current = fn;
  const optsRef = useRef(opts);
  optsRef.current = opts;
  const run = useCallback(async (...args: A): Promise<R | undefined> => {
    setPending(true);
    try {
      const r = await fnRef.current(...args);
      const s = optsRef.current.success;
      if (s) toast(typeof s === "function" ? s(r, ...args) : s);
      return r;
    } catch (e) {
      const prefix = optsRef.current.errorPrefix;
      toast(prefix ? `${prefix}: ${errorMessage(e)}` : errorMessage(e), "crit");
      return undefined;
    } finally {
      setPending(false);
    }
  }, []);
  return { run, pending };
}
