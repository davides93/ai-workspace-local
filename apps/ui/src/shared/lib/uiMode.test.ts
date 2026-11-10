import { describe, expect, it } from "vitest";
import { DEFAULT_UI_MODE, isUiMode } from "./uiMode";

describe("ui mode", () => {
  it("is simple by default", () => {
    expect(DEFAULT_UI_MODE).toBe("simple");
  });
  it("accepts only the two known modes", () => {
    expect(isUiMode("simple")).toBe(true);
    expect(isUiMode("developer")).toBe(true);
    expect(isUiMode("expert")).toBe(false);
    expect(isUiMode(undefined)).toBe(false);
  });
});
