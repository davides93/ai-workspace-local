import { useEffect, useRef } from "react";

export interface HotkeySpec {
  key: string;
  /** Ctrl on Windows/Linux, Cmd on macOS. */
  mod?: boolean;
  shift?: boolean;
}

/** Does this keyboard event match the spec? Pure, so it can be tested. */
export function matchesHotkey(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">, spec: HotkeySpec): boolean {
  if (e.key.toLowerCase() !== spec.key.toLowerCase()) return false;
  if (!!spec.mod !== (e.ctrlKey || e.metaKey)) return false;
  if (!!spec.shift !== e.shiftKey) return false;
  return !e.altKey;
}

/** Global keyboard shortcut. The handler is always the latest one passed. */
export function useHotkey(spec: HotkeySpec, handler: (e: KeyboardEvent) => void): void {
  const ref = useRef(handler);
  ref.current = handler;
  const { key, mod, shift } = spec;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (matchesHotkey(e, { key, mod, shift })) ref.current(e);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [key, mod, shift]);
}
