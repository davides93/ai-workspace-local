import type { AgentProfile } from "../api/types";
import type { UiMode } from "./uiMode";

/** The agent of the simple interface: the one chat, which delegates to the others. Seeded by the engine (`assistant`). */
export const SIMPLE_AGENT_ID = "assistant";

/** Agent preselected for a new session: the configured default (`agent.default`), else the operator, else the first. */
export function defaultAgentId(agents: AgentProfile[], configured?: unknown): string {
  return agents.find((a) => a.id === configured)?.id ?? agents.find((a) => a.id === "operator")?.id ?? agents[0]?.id ?? "";
}

/** Agent of a new session in the simple interface: the Assistente, and the usual default when the server does not have it (an older one). */
export function simpleAgentId(agents: AgentProfile[], configured?: unknown): string {
  return agents.some((a) => a.id === SIMPLE_AGENT_ID) ? SIMPLE_AGENT_ID : defaultAgentId(agents, configured);
}

/** Agent of a new session in `mode`: simple always starts with the Assistente, with no choice offered; developer keeps the configured default. */
export const newSessionAgentId = (agents: AgentProfile[], configured: unknown, mode: UiMode): string =>
  mode === "simple" ? simpleAgentId(agents, configured) : defaultAgentId(agents, configured);
