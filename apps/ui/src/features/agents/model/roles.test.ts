import { describe, expect, it } from "vitest";
import { blankAgent, normalizeAgent } from "./agents";
import { normalizeRoles, rolesMode, rolesSummary, withRole, withRolesMode } from "./roles";

const agent = () => blankAgent("auto:agent");

describe("agent roles", () => {
  it("no roles means one model, and the old JSON shape stays valid", () => {
    expect(rolesMode(undefined)).toBe("single");
    expect(rolesMode("auto")).toBe("auto");
    expect(rolesMode({ plan: "x" })).toBe("custom");
    expect("roles" in normalizeAgent(agent())).toBe(false);
  });

  it("switching the mode keeps or drops the custom picks", () => {
    const custom = withRole(withRolesMode(agent(), "custom"), "plan", "auto:reasoning");
    expect(custom.roles).toEqual({ plan: "auto:reasoning" });
    expect(withRolesMode(custom, "auto").roles).toBe("auto");
    expect("roles" in withRolesMode(custom, "single")).toBe(false);
    // Back to custom after auto starts empty (the picks went with the mode); within custom they stay.
    expect(withRolesMode(withRolesMode(custom, "auto"), "custom").roles).toEqual({});
    expect(withRolesMode(custom, "custom").roles).toEqual({ plan: "auto:reasoning" });
  });

  it("an empty pick removes the role and an empty custom plan is saved as no roles", () => {
    const a = withRole(withRole(withRolesMode(agent(), "custom"), "act", "fast"), "act", "");
    expect(a.roles).toEqual({});
    expect(normalizeRoles(a.roles)).toBeUndefined();
    expect("roles" in normalizeAgent(a)).toBe(false);
    expect(normalizeAgent({ ...agent(), roles: "auto" }).roles).toBe("auto");
    expect(normalizeAgent({ ...agent(), roles: { check: "x", plan: "" } }).roles).toEqual({ check: "x" });
  });

  it("summarises the setting in a few words", () => {
    expect(rolesSummary(undefined)).toBe("Un solo modello");
    expect(rolesSummary("auto")).toBe("Ruoli automatici");
    expect(rolesSummary({ plan: "a", check: "b" })).toBe("Ruoli personalizzati (2)");
  });
});
