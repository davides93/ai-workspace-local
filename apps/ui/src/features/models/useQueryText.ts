import { useEffect, useRef, useState } from "react";
import { setQuery, useQueryParams } from "../../app/router";

/** Pause after the last keystroke before the text reaches the URL (and with it the search request). */
export const SEARCH_DEBOUNCE_MS = 300;

/**
 * A text box mirrored into one query parameter, so the filter is shareable and survives a reload.
 * The box updates at once; the URL follows `delay` ms after the last keystroke (replacing the history
 * entry, so Back does not step through every letter). A navigation that changes the parameter from
 * outside (a link with `?q=…`) updates the box too.
 */
export function useQueryText(key: string, delay: number = SEARCH_DEBOUNCE_MS): [string, (value: string) => void] {
  const fromUrl = useQueryParams()[key] ?? "";
  const [text, setText] = useState(fromUrl);
  // Last value this hook wrote to (or read from) the URL: tells our own writes from outside ones.
  const synced = useRef(fromUrl);

  useEffect(() => {
    if (fromUrl !== synced.current) {
      synced.current = fromUrl;
      setText(fromUrl);
    }
  }, [fromUrl]);

  useEffect(() => {
    if (text === synced.current) return;
    const t = setTimeout(() => {
      synced.current = text;
      setQuery({ [key]: text });
    }, delay);
    return () => clearTimeout(t);
  }, [text, key, delay]);

  return [text, setText];
}
