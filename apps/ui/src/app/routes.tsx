import { lazy, type ComponentType, type LazyExoticComponent } from "react";
import { lastChatSession } from "../shared/lib/navMemory";
import type { RoutePattern } from "./router";
import type { IconName } from "../shared/ui";

/**
 * THE route table. Rail, tab bar, "Altro" sheet, command palette and view rendering are all
 * generated from it: adding a section means adding one entry here (and its view folder).
 */

export type RouteId =
  | "home" | "chat" | "inbox" | "agents" | "teach" | "teams" | "workflows" | "evals" | "training" | "capabilities" | "integrations"
  | "models" | "runtimes" | "devices" | "security" | "activity" | "settings";

export type GroupId = "overview" | "work" | "capabilities" | "inference" | "control";

export interface AppRoute extends RoutePattern {
  id: RouteId;
  label: string;
  icon: IconName;
  group: GroupId;
  /** Position in the phone tab bar (the fifth slot is always "Altro"). */
  tab?: number;
  /** Position in the phone tab bar in simple mode, when it differs from `tab`. */
  simpleTab?: number;
  /** Shown in simple mode (`ui.mode`); developer mode shows every route. */
  simple?: boolean;
  /** Plainer name used in simple mode. */
  simpleLabel?: string;
  /** Extra words the command palette matches. */
  keywords: string[];
  View: LazyExoticComponent<ComponentType>;
}

export const GROUPS: { id: GroupId; label?: string; simpleLabel?: string }[] = [
  { id: "overview" },
  { id: "work", label: "Lavoro" },
  { id: "capabilities", label: "Capacità" },
  { id: "inference", label: "Inferenza", simpleLabel: "Intelligenza" },
  { id: "control", label: "Controllo" },
];

const view = <K extends string>(load: () => Promise<Record<K, ComponentType>>, name: K) =>
  lazy(() => load().then((m) => ({ default: m[name] })));

export const ROUTES: AppRoute[] = [
  { id: "home", path: "/", label: "Panoramica", icon: "home", group: "overview", tab: 0, simple: true, keywords: ["home", "dashboard", "inizio"], View: view(() => import("../features/home/HomeView"), "HomeView") },
  { id: "chat", path: "/chat/:sessionId?", label: "Workspace", simpleLabel: "Chat", icon: "chat", group: "work", tab: 1, simple: true, keywords: ["chat", "workspace", "conversazione", "sessioni"], View: view(() => import("../features/chat/ChatView"), "ChatView") },
  { id: "inbox", path: "/inbox", label: "Proposte", icon: "sparkle", group: "work", simple: true, keywords: ["proposte", "inbox", "suggerimenti", "promemoria", "riepilogo", "proattivo", "notato"], View: view(() => import("../features/inbox/InboxView"), "InboxView") },
  { id: "agents", path: "/agents/:agentId?", label: "Agenti", simpleLabel: "Assistenti", icon: "agent", group: "work", simple: true, keywords: ["agenti", "assistenti", "profili", "prompt"], View: view(() => import("../features/agents/AgentsView"), "AgentsView") },
  { id: "teach", path: "/teach", label: "Insegna", icon: "bulb", group: "work", simpleTab: 2, simple: true, keywords: ["insegna", "memoria", "ricorda", "ricordi", "cosa sai di me", "preferenze", "impara"], View: view(() => import("../features/teach/TeachView"), "TeachView") },
  { id: "teams", path: "/teams/:teamId?", label: "Team", icon: "team", group: "work", keywords: ["squadra", "supervisore", "multi-agente", "parallelo", "catena"], View: view(() => import("../features/teams/TeamsView"), "TeamsView") },
  { id: "workflows", path: "/workflows/:workflowId?", label: "Workflow", icon: "flow", group: "work", keywords: ["automazioni", "trigger", "webhook"], View: view(() => import("../features/workflows/WorkflowsView"), "WorkflowsView") },
  { id: "evals", path: "/evals/:tab?", label: "Valutazioni", icon: "check", group: "work", keywords: ["valutazione", "eval", "test", "benchmark", "grader", "confronto", "qualità"], View: view(() => import("../features/evals/EvalsView"), "EvalsView") },
  { id: "capabilities", path: "/capabilities/:tab?", label: "Tool, MCP e browser", icon: "tool", group: "capabilities", keywords: ["tool", "mcp", "browser", "computer-use", "strumenti"], View: view(() => import("../features/capabilities/CapabilitiesView"), "CapabilitiesView") },
  { id: "integrations", path: "/integrations", label: "Integrazioni", icon: "plug", group: "capabilities", keywords: ["api", "openai", "anthropic", "a2a", "webhook", "endpoint"], View: view(() => import("../features/integrations/IntegrationsView"), "IntegrationsView") },
  { id: "models", path: "/models/:tab?", label: "Modelli", icon: "cube", group: "inference", tab: 2, simpleTab: 3, simple: true, keywords: ["modelli", "scarica", "gguf", "hugging face", "ollama", "download"], View: view(() => import("../features/models/ModelsView"), "ModelsView") },
  { id: "runtimes", path: "/runtimes/:runtimeId?", label: "Runtime", icon: "chip", group: "inference", tab: 3, keywords: ["llama", "motore", "inferenza"], View: view(() => import("../features/runtimes/RuntimesView"), "RuntimesView") },
  { id: "devices", path: "/devices", label: "Dispositivi", icon: "device", group: "inference", keywords: ["hardware", "pairing", "gpu", "rete"], View: view(() => import("../features/devices/DevicesView"), "DevicesView") },
  { id: "training", path: "/training/:tab?", label: "Addestramento", icon: "bolt", group: "inference", keywords: ["fine-tuning", "lora", "adapter", "sft", "dpo", "grpo", "dataset", "addestra"], View: view(() => import("../features/training/TrainingView"), "TrainingView") },
  { id: "security", path: "/security/:tab?", label: "Sicurezza", icon: "shield", group: "control", keywords: ["policy", "hook", "sandbox", "permessi"], View: view(() => import("../features/security/SecurityView"), "SecurityView") },
  { id: "activity", path: "/activity", label: "Attività", icon: "activity", group: "control", keywords: ["audit", "log", "cronologia"], View: view(() => import("../features/activity/ActivityView"), "ActivityView") },
  { id: "settings", path: "/settings", label: "Impostazioni", icon: "settings", group: "control", simple: true, keywords: ["configurazione", "tema", "preferenze"], View: view(() => import("../features/settings/SettingsView"), "SettingsView") },
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
