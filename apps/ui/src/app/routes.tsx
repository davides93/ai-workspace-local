import { lazy, type ComponentType, type LazyExoticComponent } from "react";
import { lastChatSession } from "../shared/lib/navMemory";
import type { RoutePattern } from "./router";
import type { MessageKey } from "../shared/i18n";
import type { IconName } from "../shared/ui";

/**
 * THE route table. Rail, tab bar, "Altro" sheet, command palette and view rendering are all
 * generated from it: adding a section means adding one entry here (and its view folder).
 */

export type RouteId =
  | "home" | "chat" | "inbox" | "agents" | "teach" | "documents" | "channels" | "teams" | "workflows" | "evals" | "training" | "capabilities" | "integrations"
  | "models" | "runtimes" | "devices" | "security" | "activity" | "settings";

export type GroupId = "overview" | "work" | "capabilities" | "inference" | "control";

export interface AppRoute extends RoutePattern {
  id: RouteId;
  /** Message key of the section name. */
  label: MessageKey;
  icon: IconName;
  group: GroupId;
  /** Position in the phone tab bar (the fifth slot is always "Altro"). */
  tab?: number;
  /** Position in the phone tab bar in simple mode, when it differs from `tab`. */
  simpleTab?: number;
  /** Shown in simple mode (`ui.mode`); developer mode shows every route. */
  simple?: boolean;
  /** Plainer name used in simple mode. */
  simpleLabel?: MessageKey;
  /** Message key of the extra words (comma separated) the command palette matches. */
  keywords: MessageKey;
  View: LazyExoticComponent<ComponentType>;
}

export const GROUPS: { id: GroupId; label?: MessageKey; simpleLabel?: MessageKey }[] = [
  { id: "overview" },
  { id: "work", label: "nav.group_work" },
  { id: "capabilities", label: "nav.group_capabilities" },
  { id: "inference", label: "nav.group_inference", simpleLabel: "nav.group_inference_simple" },
  { id: "control", label: "nav.group_control" },
];

const view = <K extends string>(load: () => Promise<Record<K, ComponentType>>, name: K) =>
  lazy(() => load().then((m) => ({ default: m[name] })));

export const ROUTES: AppRoute[] = [
  { id: "home", path: "/", label: "nav.home", icon: "home", group: "overview", tab: 0, simple: true, keywords: "nav.home_keywords", View: view(() => import("../features/home/HomeView"), "HomeView") },
  { id: "chat", path: "/chat/:sessionId?", label: "nav.chat", simpleLabel: "nav.chat_simple", icon: "chat", group: "work", tab: 1, simple: true, keywords: "nav.chat_keywords", View: view(() => import("../features/chat/ChatView"), "ChatView") },
  { id: "inbox", path: "/inbox", label: "nav.inbox", icon: "sparkle", group: "work", simple: true, keywords: "nav.inbox_keywords", View: view(() => import("../features/inbox/InboxView"), "InboxView") },
  { id: "agents", path: "/agents/:agentId?", label: "nav.agents", simpleLabel: "nav.agents_simple", icon: "agent", group: "work", simple: true, keywords: "nav.agents_keywords", View: view(() => import("../features/agents/AgentsView"), "AgentsView") },
  { id: "teach", path: "/teach", label: "nav.teach", icon: "bulb", group: "work", simpleTab: 2, simple: true, keywords: "nav.teach_keywords", View: view(() => import("../features/teach/TeachView"), "TeachView") },
  { id: "documents", path: "/documents", label: "nav.documents", icon: "file", group: "work", simple: true, keywords: "nav.documents_keywords", View: view(() => import("../features/documents/DocumentsView"), "DocumentsView") },
  { id: "channels", path: "/channels", label: "nav.channels", simpleLabel: "nav.channels_simple", icon: "send", group: "work", simple: true, keywords: "nav.channels_keywords", View: view(() => import("../features/channels/ChannelsView"), "ChannelsView") },
  { id: "teams", path: "/teams/:teamId?", label: "nav.teams", icon: "team", group: "work", keywords: "nav.teams_keywords", View: view(() => import("../features/teams/TeamsView"), "TeamsView") },
  { id: "workflows", path: "/workflows/:workflowId?", label: "nav.workflows", icon: "flow", group: "work", keywords: "nav.workflows_keywords", View: view(() => import("../features/workflows/WorkflowsView"), "WorkflowsView") },
  { id: "evals", path: "/evals/:tab?", label: "nav.evals", icon: "check", group: "work", keywords: "nav.evals_keywords", View: view(() => import("../features/evals/EvalsView"), "EvalsView") },
  { id: "capabilities", path: "/capabilities/:tab?", label: "nav.capabilities", icon: "tool", group: "capabilities", keywords: "nav.capabilities_keywords", View: view(() => import("../features/capabilities/CapabilitiesView"), "CapabilitiesView") },
  { id: "integrations", path: "/integrations", label: "nav.integrations", icon: "plug", group: "capabilities", keywords: "nav.integrations_keywords", View: view(() => import("../features/integrations/IntegrationsView"), "IntegrationsView") },
  { id: "models", path: "/models/:tab?", label: "nav.models", icon: "cube", group: "inference", tab: 2, simpleTab: 3, simple: true, keywords: "nav.models_keywords", View: view(() => import("../features/models/ModelsView"), "ModelsView") },
  { id: "runtimes", path: "/runtimes/:runtimeId?", label: "nav.runtimes", icon: "chip", group: "inference", tab: 3, keywords: "nav.runtimes_keywords", View: view(() => import("../features/runtimes/RuntimesView"), "RuntimesView") },
  { id: "devices", path: "/devices", label: "nav.devices", icon: "device", group: "inference", keywords: "nav.devices_keywords", View: view(() => import("../features/devices/DevicesView"), "DevicesView") },
  { id: "training", path: "/training/:tab?", label: "nav.training", icon: "bolt", group: "inference", keywords: "nav.training_keywords", View: view(() => import("../features/training/TrainingView"), "TrainingView") },
  { id: "security", path: "/security/:tab?", label: "nav.security", icon: "shield", group: "control", keywords: "nav.security_keywords", View: view(() => import("../features/security/SecurityView"), "SecurityView") },
  { id: "activity", path: "/activity", label: "nav.activity", icon: "activity", group: "control", keywords: "nav.activity_keywords", View: view(() => import("../features/activity/ActivityView"), "ActivityView") },
  { id: "settings", path: "/settings", label: "nav.settings", icon: "settings", group: "control", simple: true, keywords: "nav.settings_keywords", View: view(() => import("../features/settings/SettingsView"), "SettingsView") },
];

export const routeById = (id: RouteId): AppRoute => ROUTES.find((r) => r.id === id)!;

/** Phone tab bar entries, in order. */
export const TAB_ROUTES: AppRoute[] = ROUTES.filter((r) => r.tab !== undefined).sort((a, b) => (a.tab ?? 0) - (b.tab ?? 0));

/** Where a navigation entry for this route leads: the chat reopens the last active session. */
export function routeHref(r: Pick<AppRoute, "id" | "path">): string {
  if (r.id === "chat") {
    const last = lastChatSession();
    return last ? `/chat/${encodeURIComponent(last)}` : "/chat";
  }
  return r.path.replace(/\/:[A-Za-z]+\?$/, "") || "/";
}
