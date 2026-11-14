import { describe, expect, it } from "vitest";
import type { AgentProfile } from "../api/types";
import { defaultAgentId, newSessionAgentId, SIMPLE_AGENT_ID, simpleAgentId } from "./agents";

const agent = (id: string): AgentProfile => ({ id, name: id, description: "", inference_profile: "auto:agent", system_prompt: "", tools: [], mode: "ask", sandbox: "workspace-write", max_turns: 10 });
const server = [agent("operator"), agent("coder"), agent("assistant")];
const older = [agent("operator"), agent("coder")];

describe("agents of a new session", () => {
  it("names the Assistente as the agent of the simple interface", () => {
    expect(SIMPLE_AGENT_ID).toBe("assistant");
  });
  it("uses the Assistente in simple mode whatever the configured default is", () => {
    expect(simpleAgentId(server, "coder")).toBe("assistant");
    expect(newSessionAgentId(server, "coder", "simple")).toBe("assistant");
    expect(newSessionAgentId(server, undefined, "simple")).toBe("assistant");
  });
  it("falls back to the usual default when the server has no Assistente", () => {
    expect(simpleAgentId(older, "coder")).toBe("coder");
    expect(simpleAgentId(older, undefined)).toBe("operator");
    expect(newSessionAgentId(older, undefined, "simple")).toBe("operator");
  });
  it("gives no agent until the agents have arrived", () => {
    expect(simpleAgentId([], "operator")).toBe("");
    expect(newSessionAgentId([], "operator", "simple")).toBe("");
  });
  it("keeps the configured default in developer mode, as before", () => {
    expect(newSessionAgentId(server, "coder", "developer")).toBe("coder");
    expect(newSessionAgentId(server, undefined, "developer")).toBe("operator");
    expect(newSessionAgentId(server, "assistant", "developer")).toBe("assistant");
    expect(defaultAgentId(older, "unknown")).toBe("operator");
  });
});

describe("defaultAgentId", () => {
  const agents = [{ id: "coder" }, { id: "operator" }] as AgentProfile[];
  it("prefers the configured agent, then the operator, then the first", () => {
    expect(defaultAgentId(agents, "coder")).toBe("coder");
    expect(defaultAgentId(agents, "missing")).toBe("operator");
    expect(defaultAgentId([{ id: "x" }] as AgentProfile[])).toBe("x");
    expect(defaultAgentId([])).toBe("");
  });
});
