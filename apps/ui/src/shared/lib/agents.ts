import type { AgentProfile } from "../api/types";

/** Agent preselected for a new session: the configured default (`agent.default`), else the operator, else the first. */
export function defaultAgentId(agents: AgentProfile[], configured?: unknown): string {
  return agents.find((a) => a.id === configured)?.id ?? agents.find((a) => a.id === "operator")?.id ?? agents[0]?.id ?? "";
}
