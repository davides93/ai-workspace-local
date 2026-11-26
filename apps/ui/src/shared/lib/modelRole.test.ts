import { describe, expect, it } from "vitest";
import { modelRoleId, modelRoleLabel, profileRoleLabel } from "./modelRole";
import type { LocalModel } from "../api/types";

const m = (over: Partial<LocalModel>) => ({ id: "x", name: "X", format: "gguf", params_b: 4, ...over }) as LocalModel;

describe("model role", () => {
  it("comes from a system model, else from the size", () => {
    expect(modelRoleId(m({ format: "system", params_b: undefined }))).toBe("system");
    expect(modelRoleId(m({ params_b: 1.7 }))).toBe("light");
    expect(modelRoleId(m({ params_b: 3 }))).toBe("light");
    expect(modelRoleId(m({ params_b: 4.7 }))).toBe("fast");
    expect(modelRoleId(m({ params_b: 9 }))).toBe("fast");
    expect(modelRoleId(m({ params_b: 26.4 }))).toBe("smart");
  });

  it("is unknown when the model is missing or has no size", () => {
    expect(modelRoleId(undefined)).toBeUndefined();
    expect(modelRoleId(m({ params_b: undefined }))).toBeUndefined();
    expect(modelRoleId(m({ params_b: 0 }))).toBeUndefined();
  });

  it("is looked up from the profile's model id", () => {
    const models = [m({ id: "a", params_b: 27 })];
    expect(profileRoleLabel({ model: "a" }, models)).toBe(modelRoleLabel("smart"));
    expect(profileRoleLabel({ model: "b" }, models)).toBeUndefined();
    expect(profileRoleLabel(null, models)).toBeUndefined();
  });
});
