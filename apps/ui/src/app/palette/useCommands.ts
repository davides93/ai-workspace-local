import { useMemo } from "react";
import { useRefreshRuntimes } from "../../features/runtimes/api";
import { useAgents } from "../../features/agents/api";
import { useCreateSession } from "../../features/chat/api";
import { setTheme } from "../../shared/lib/theme";
import { useKill } from "../../shared/query/system";
import { navigate } from "../router";
import { groupLabel, routeLabel, visibleRoutes } from "../navigation";
import { GROUPS, ROUTES, routeHref } from "../routes";
import { useUiMode } from "../useUiMode";
import type { Command } from "./commands";

/**
 * Sections and actions of the command palette. Called by the shell (which never unmounts), so a
 * mutation started from the palette still reports back after the palette has closed.
 */
export function useCommands(): Command[] {
  const agents = useAgents();
  const createSession = useCreateSession();
  const refresh = useRefreshRuntimes();
  const kill = useKill();
  const [mode, setMode] = useUiMode();
  return useMemo<Command[]>(() => {
    const groupHint = (id: string) => { const g = GROUPS.find((x) => x.id === id); return g ? groupLabel(g, mode) : undefined; };
    const nav: Command[] = visibleRoutes(ROUTES, mode).map((r) => ({
      id: `go-${r.id}`, label: routeLabel(r, mode), hint: groupHint(r.group), group: "Vai a", icon: r.icon, keywords: r.keywords,
      run: () => navigate(routeHref(r)),
    }));
    const actions: Command[] = [
      {
        id: "new-session", label: "Nuova sessione", group: "Azioni", icon: "plus", keywords: ["chat", "conversazione", "inizia"],
        run: () => {
          const agent = agents.data?.find((a) => a.id === "operator") ?? agents.data?.[0];
          if (!agent) return navigate("/agents");
          createSession.mutate({ agent_id: agent.id }, { onSuccess: (s) => navigate(`/chat/${encodeURIComponent(s.id)}`) });
        },
      },
      { id: "download-model", label: "Scarica un modello", group: "Azioni", icon: "download", keywords: ["modello", "scopri", "hugging face", "ollama", "gguf"], run: () => navigate("/models/discover") },
      ...(mode === "advanced" ? [{ id: "detect-runtimes", label: "Rileva runtime", group: "Azioni" as const, icon: "refresh", keywords: ["runtime", "cerca", "aggiorna"], run: () => refresh.mutate() }] : []),
      mode === "simple"
        ? { id: "mode-advanced", label: "Mostra tutte le sezioni", group: "Azioni" as const, icon: "settings", keywords: ["avanzate", "modalità", "interfaccia", "tutto"], run: () => setMode("advanced") }
        : { id: "mode-simple", label: "Passa all'interfaccia semplice", group: "Azioni" as const, icon: "settings", keywords: ["semplice", "modalità", "interfaccia", "meno sezioni"], run: () => setMode("simple") },
      { id: "kill", label: "Kill switch", hint: "ferma tutti i run", group: "Azioni", icon: "power", keywords: ["stop", "emergenza", "ferma", "arresta"], run: () => kill.mutate() },
      { id: "theme-light", label: "Tema chiaro", group: "Azioni", icon: "sun", keywords: ["aspetto", "colori"], run: () => setTheme("light") },
      { id: "theme-dark", label: "Tema scuro", group: "Azioni", icon: "moon", keywords: ["aspetto", "colori", "notte"], run: () => setTheme("dark") },
      { id: "theme-system", label: "Tema del sistema", group: "Azioni", icon: "settings", keywords: ["aspetto", "automatico"], run: () => setTheme("system") },
    ];
    return [...nav, ...actions];
  }, [agents.data, createSession, refresh, kill, mode, setMode]);
}
