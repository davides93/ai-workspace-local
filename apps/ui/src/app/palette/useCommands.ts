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
import { splitKeywords, type Command } from "./commands";
import { t, t as tr } from "../../shared/i18n";

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
      id: `go-${r.id}`, label: routeLabel(r, mode), hint: groupHint(r.group), group: "go", icon: r.icon, keywords: splitKeywords(t(r.keywords)),
      run: () => navigate(routeHref(r)),
    }));
    const actions: Command[] = [
      {
        id: "new-session", label: t("shell.cmd_new_session"), group: "actions", icon: "plus", keywords: splitKeywords(t("shell.cmd_new_session_kw")),
        run: () => {
          const agent = agents.data?.find((a) => a.id === "operator") ?? agents.data?.[0];
          if (!agent) return navigate("/agents");
          createSession.mutate({ agent_id: agent.id }, { onSuccess: (s) => navigate(`/chat/${encodeURIComponent(s.id)}`) });
        },
      },
      { id: "download-model", label: t("shell.cmd_download_model"), group: "actions", icon: "download", keywords: splitKeywords(t("shell.cmd_download_model_kw")), run: () => navigate("/models/discover") },
      ...(mode === "developer" ? [{ id: "detect-runtimes", label: t("shell.cmd_detect_runtimes"), group: "actions" as const, icon: "refresh", keywords: splitKeywords(t("shell.cmd_detect_runtimes_kw")), run: () => refresh.mutate() }] : []),
      mode === "simple"
        ? { id: "mode-developer", label: t("shell.mostra_tutte_le_sezioni"), group: "actions" as const, icon: "settings", keywords: splitKeywords(t("shell.cmd_developer_kw")), run: () => setMode("developer") }
        : { id: "mode-simple", label: t("shell.cmd_simple_mode"), group: "actions" as const, icon: "settings", keywords: splitKeywords(t("shell.cmd_simple_kw")), run: () => setMode("simple") },
      { id: "kill", label: tr("shell.kill_switch"), hint: t("shell.cmd_kill_hint"), group: "actions", icon: "power", keywords: splitKeywords(t("shell.cmd_kill_kw")), run: () => kill.mutate() },
      { id: "theme-light", label: t("shell.cmd_theme_light"), group: "actions", icon: "sun", keywords: splitKeywords(t("shell.cmd_theme_kw")), run: () => setTheme("light") },
      { id: "theme-dark", label: t("shell.cmd_theme_dark"), group: "actions", icon: "moon", keywords: splitKeywords(t("shell.cmd_theme_dark_kw")), run: () => setTheme("dark") },
      { id: "theme-system", label: t("shell.cmd_theme_system"), group: "actions", icon: "settings", keywords: splitKeywords(t("shell.cmd_theme_system_kw")), run: () => setTheme("system") },
    ];
    return [...nav, ...actions];
  }, [agents.data, createSession, refresh, kill, mode, setMode]);
}
