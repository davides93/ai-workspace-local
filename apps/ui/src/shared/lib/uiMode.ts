import { useSyncExternalStore } from "react";
import { labelMap } from "../i18n";

/** `simple` shows a handful of sections; `developer` shows everything (setting `ui.mode`). */
export type UiMode = "simple" | "developer";

export const UI_MODES: UiMode[] = ["simple", "developer"];
export const DEFAULT_UI_MODE: UiMode = "simple";
export const UI_MODE_LABEL: Record<UiMode, string> = labelMap({ simple: "shell.mode_simple", developer: "shell.mode_developer" });

export const isUiMode = (v: unknown): v is UiMode => v === "simple" || v === "developer";

const KEY = "aiw.ui.mode";
const listeners = new Set<() => void>();
let current: UiMode = read();

/** Last mode seen on this device: the shell renders with it until the configuration has answered. */
function read(): UiMode {
  try {
    const v = localStorage.getItem(KEY);
    return isUiMode(v) ? v : DEFAULT_UI_MODE;
  } catch {
    return DEFAULT_UI_MODE;
  }
}

export function setStoredUiMode(mode: UiMode): void {
  if (mode === current) return;
  current = mode;
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    /* storage unavailable: the mode lasts until the page is reloaded */
  }
  for (const l of listeners) l();
}

/** The mode this device shows now (the configuration is the source of truth, see `useUiMode`). */
export function useStoredUiMode(): UiMode {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => {
        listeners.delete(l);
      };
    },
    () => current,
    () => DEFAULT_UI_MODE,
  );
}
