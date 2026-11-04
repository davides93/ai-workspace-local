import { describe, expect, it } from "vitest";
import type { AgentProfile } from "../api/types";
import { defaultAgentId } from "./agents";

describe("defaultAgentId", () => {
  const agents = [{ id: "coder" }, { id: "operator" }] as AgentProfile[];
  it("prefers the configured agent, then the operator, then the first", () => {
    expect(defaultAgentId(agents, "coder")).toBe("coder");
    expect(defaultAgentId(agents, "missing")).toBe("operator");
    expect(defaultAgentId([{ id: "x" }] as AgentProfile[])).toBe("x");
    expect(defaultAgentId([])).toBe("");
  });
});
