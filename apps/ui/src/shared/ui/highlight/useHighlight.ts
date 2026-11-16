import { useEffect, useMemo, useState } from "react";
import { grammarFor } from "./languages";
import type { Token } from "./tokenize";

type Engine = typeof import("./tokenize");

let engine: Engine | undefined;
let loading: Promise<Engine> | undefined;

/** Fetches the grammars once (a separate chunk); a failed fetch is tried again the next time a block asks. */
export function loadHighlighter(): Promise<Engine> {
  loading ??= import("./tokenize").then(
    (m) => (engine = m),
    (e: unknown) => {
      loading = undefined;
      throw e;
    },
  );
  return loading;
}

/**
 * The coloured runs of a code block, or `null` while it stays plain: the language has no grammar, or the grammars have not
 * arrived yet (the block is shown plain first and coloured when they do; the text does not change, so nothing moves).
 */
export function useHighlight(code: string, lang: string | undefined): Token[] | null {
  const grammar = grammarFor(lang);
  const [loaded, setLoaded] = useState<Engine | undefined>(engine);
  useEffect(() => {
    if (!grammar || loaded) return;
    let alive = true;
    loadHighlighter().then(
      (m) => alive && setLoaded(m),
      () => {
        /* stays plain */
      },
    );
    return () => {
      alive = false;
    };
  }, [grammar, loaded]);
  return useMemo(() => {
    if (!grammar || !loaded) return null;
    const tokens = loaded.tokenize(code, grammar);
    return tokens.some((t) => t.k !== null) ? tokens : null;
  }, [grammar, loaded, code]);
}
