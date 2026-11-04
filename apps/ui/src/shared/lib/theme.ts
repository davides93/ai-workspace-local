import { useCallback, useSyncExternalStore } from "react";

export type Theme = "system" | "light" | "dark";

const KEY = "aiw.theme";
const listeners = new Set<() => void>();
let current: Theme = read();
/** True once this module wrote `data-theme` itself (an embedding host may own it until then). */
let owned = false;

function read(): Theme {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" || v === "system" ? v : "system";
  } catch {
    return "system";
  }
}

const media = () => (typeof window !== "undefined" && typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: dark)") : null);

/** "system" becomes the OS preference; the stylesheet only knows `light` (base) and `dark` (one block). */
export function resolveTheme(theme: Theme, prefersDark: boolean): "light" | "dark" {
  return theme === "system" ? (prefersDark ? "dark" : "light") : theme;
}

function apply(theme: Theme) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  // In "system" mode leave a host-provided attribute alone (e.g. a viewer frame forcing a theme).
  if (theme === "system" && !owned && root.hasAttribute("data-theme")) return;
  root.setAttribute("data-theme", resolveTheme(theme, media()?.matches ?? false));
  owned = true;
}

/** Applies the stored theme at boot and follows the OS while the theme is "system". */
export function initTheme(): void {
  apply(current);
  media()?.addEventListener("change", () => {
    if (current === "system" && owned) apply("system");
  });
}

export function setTheme(theme: Theme): void {
  current = theme;
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    /* storage unavailable */
  }
  apply(theme);
  for (const l of listeners) l();
}

export function useTheme(): [Theme, (t: Theme) => void] {
  const theme = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => {
        listeners.delete(l);
      };
    },
    () => current,
    () => "system" as Theme,
  );
  return [theme, useCallback((t: Theme) => setTheme(t), [])];
}
