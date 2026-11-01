import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type { Transport } from "../api/transport";
import type { SystemInfo } from "../api/types";

export type Route = "chat" | "agents" | "workflows" | "runtimes" | "models" | "devices" | "tools" | "activity" | "preferences";

interface Toast { id: number; text: string; tone?: "crit" }

interface AppCtx {
  api: Transport;
  system: SystemInfo | null;
  needsPairing: boolean;
  refreshSystem: () => void;
  route: Route;
  go: (r: Route) => void;
  toast: (text: string, tone?: "crit") => void;
  notice?: string;
  theme: "system" | "light" | "dark";
  setTheme: (t: "system" | "light" | "dark") => void;
}

const Ctx = createContext<AppCtx | null>(null);
export const useApp = () => {
  const c = useContext(Ctx);
  if (!c) throw new Error("AppProvider missing");
  return c;
};

const ROUTES: Route[] = ["chat", "agents", "workflows", "runtimes", "models", "devices", "tools", "activity", "preferences"];
const readRoute = (): Route => {
  const h = location.hash.replace(/^#/, "") as Route;
  return ROUTES.includes(h) ? h : "chat";
};

export function AppProvider({ api, children }: { api: Transport; children: ReactNode }) {
  const [system, setSystem] = useState<SystemInfo | null>(null);
  const [needsPairing, setNeedsPairing] = useState(false);
  const [sysTick, setSysTick] = useState(0);
  const [route, setRoute] = useState<Route>(readRoute);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [theme, setThemeState] = useState<"system" | "light" | "dark">(() => {
    try { return (localStorage.getItem("aiw.theme") as "system" | "light" | "dark") || "system"; } catch { return "system"; }
  });

  useEffect(() => {
    const load = () =>
      api
        .system()
        .then((s) => {
          setSystem(s);
          setNeedsPairing(false);
        })
        .catch((e) => {
          if ((e as { status?: number }).status === 401) setNeedsPairing(true);
        });
    load();
    const t = setInterval(load, 15000);
    return () => clearInterval(t);
  }, [api, sysTick]);

  useEffect(() => {
    const onHash = () => setRoute(readRoute());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const [themeTouched, setThemeTouched] = useState(false);
  useEffect(() => {
    const root = document.documentElement;
    // In "system" mode leave the attribute alone unless the user just switched to it:
    // an embedding host (e.g. a viewer frame) may own data-theme.
    if (theme !== "system") root.setAttribute("data-theme", theme);
    else if (themeTouched) root.removeAttribute("data-theme");
    try { localStorage.setItem("aiw.theme", theme); } catch { /* storage unavailable */ }
  }, [theme, themeTouched]);

  const go = useCallback((r: Route) => {
    if (location.hash !== `#${r}`) location.hash = r;
    setRoute(r);
  }, []);

  const toast = useCallback((text: string, tone?: "crit") => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t, { id, text, tone }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 3800);
  }, []);

  const value = useMemo<AppCtx>(() => ({
    api, system, route, go, toast, theme, needsPairing, refreshSystem: () => setSysTick((x) => x + 1),
    setTheme: (t: "system" | "light" | "dark") => { setThemeTouched(true); setThemeState(t); },
    notice: (api as unknown as { notice?: string }).notice,
  }), [api, system, route, go, toast, theme, needsPairing]);

  return (
    <Ctx.Provider value={value}>
      {children}
      <div className="toasts" aria-live="polite">
        {toasts.map((t) => <div key={t.id} className={`toast ${t.tone ?? ""}`}>{t.text}</div>)}
      </div>
    </Ctx.Provider>
  );
}
