import { useMemo } from "react";
import { useRefreshRuntimes } from "../../features/runtimes/api";
import { useAgents } from "../../features/agents/api";
import { useCreateSession } from "../../features/chat/api";
import { seedComposer } from "../../features/chat/model/composerSeed";
import { useConfig } from "../../features/settings/api";
import { simpleAgentId } from "../../shared/lib/agents";
import { setTheme } from "../../shared/lib/theme";
import type { UiMode } from "../../shared/lib/uiMode";
import { useKill } from "../../shared/query/system";
import type { AgentProfile } from "../../shared/api/types";
import { navigate } from "../router";
import { groupLabel, routeLabel, visibleRoutes } from "../navigation";
import { GROUPS, ROUTES, routeHref } from "../routes";
import { useUiMode } from "../useUiMode";
import { splitKeywords, type Command } from "./commands";
import { t, t as tr } from "../../shared/i18n";

/** What the commands act on: the app's state and its side effects, so that the list can be built (and tested) without React. */
export interface CommandDeps {
  mode: UiMode;
  agents: AgentProfile[] | undefined;
  /** `agent.default` of the configuration. */
  configuredAgent: unknown;
  /** Creates a session with the agent and opens it. */
  newSession: (agentId: string) => void;
  refreshRuntimes: () => void;
  kill: () => void;
  setMode: (mode: UiMode) => void;
  go: (to: string) => void;
  /** Opens the chat with this text in the message box. */
  askInChat: (text: string) => void;
}

/**
 * Sections and actions of the command palette. In the simple interface «Vai a» lists the chat only, «Nuova sessione» starts
 * one with the Assistente and «Scarica un modello» opens the chat with the request typed in the message box (the Modelli
 * page is not there); developer mode is as it always was.
 */
export function buildCommands(d: CommandDeps): Command[] {
  const { mode } = d;
  const groupHint = (id: string) => { const g = GROUPS.find((x) => x.id === id); return g ? groupLabel(g, mode) : undefined; };
  const nav: Command[] = visibleRoutes(ROUTES, mode).map((r) => ({
    id: `go-${r.id}`, label: routeLabel(r, mode), hint: groupHint(r.group), group: "go", icon: r.icon, keywords: splitKeywords(t(r.keywords)),
    run: () => d.go(routeHref(r)),
  }));
  const actions: Command[] = [
    {
      id: "new-session", label: t("shell.cmd_new_session"), group: "actions", icon: "plus", keywords: splitKeywords(t("shell.cmd_new_session_kw")),
      run: () => {
        const list = d.agents ?? [];
        const agentId = mode === "simple" ? simpleAgentId(list, d.configuredAgent) : (list.find((a) => a.id === "operator") ?? list[0])?.id;
        if (!agentId) return d.go(mode === "simple" ? "/chat" : "/agents");
        d.newSession(agentId);
      },
    },
    {
      id: "download-model", label: t("shell.cmd_download_model"), group: "actions", icon: "download", keywords: splitKeywords(t("shell.cmd_download_model_kw")),
      run: () => (mode === "simple" ? d.askInChat(t("shell.cmd_download_model_prompt")) : d.go("/models/discover")),
    },
    ...(mode === "developer" ? [{ id: "detect-runtimes", label: t("shell.cmd_detect_runtimes"), group: "actions" as const, icon: "refresh", keywords: splitKeywords(t("shell.cmd_detect_runtimes_kw")), run: d.refreshRuntimes }] : []),
    mode === "simple"
      ? { id: "mode-developer", label: t("shell.mostra_tutte_le_sezioni"), group: "actions" as const, icon: "settings", keywords: splitKeywords(t("shell.cmd_developer_kw")), run: () => d.setMode("developer") }
      : { id: "mode-simple", label: t("shell.cmd_simple_mode"), group: "actions" as const, icon: "settings", keywords: splitKeywords(t("shell.cmd_simple_kw")), run: () => d.setMode("simple") },
    { id: "kill", label: tr("shell.kill_switch"), hint: t("shell.cmd_kill_hint"), group: "actions", icon: "power", keywords: splitKeywords(t("shell.cmd_kill_kw")), run: d.kill },
    { id: "theme-light", label: t("shell.cmd_theme_light"), group: "actions", icon: "sun", keywords: splitKeywords(t("shell.cmd_theme_kw")), run: () => setTheme("light") },
    { id: "theme-dark", label: t("shell.cmd_theme_dark"), group: "actions", icon: "moon", keywords: splitKeywords(t("shell.cmd_theme_dark_kw")), run: () => setTheme("dark") },
    { id: "theme-system", label: t("shell.cmd_theme_system"), group: "actions", icon: "settings", keywords: splitKeywords(t("shell.cmd_theme_system_kw")), run: () => setTheme("system") },
  ];
  return [...nav, ...actions];
}

/**
 * The commands of the palette. Called by the shell (which never unmounts), so a mutation started from the palette still
 * reports back after the palette has closed.
 */
export function useCommands(): Command[] {
  const agents = useAgents();
  const config = useConfig();
  const createSession = useCreateSession();
  const refresh = useRefreshRuntimes();
  const kill = useKill();
  const [mode, setMode] = useUiMode();
  const configuredAgent = config.data?.effective["agent.default"];
  return useMemo(
    () => buildCommands({
      mode, agents: agents.data, configuredAgent, setMode, go: (to) => navigate(to),
      newSession: (agent_id) => createSession.mutate({ agent_id }, { onSuccess: (s) => navigate(`/chat/${encodeURIComponent(s.id)}`) }),
      refreshRuntimes: () => refresh.mutate(),
      kill: () => kill.mutate(),
      askInChat: (text) => {
        seedComposer(text);
        // Already in the chat: the open conversation takes the text; otherwise open it (it reopens the last session).
        if (!/^#\/chat(\/|\?|$)/.test(location.hash)) navigate("/chat");
      },
    }),
    [agents.data, configuredAgent, createSession, refresh, kill, mode, setMode],
  );
}
