import { describe, expect, it } from "vitest";
import { canReason, effectiveThinking, isThinkingMode, THINKING_LABEL, THINKING_MODES } from "./thinking";

describe("thinking", () => {
  it("names the three choices in Italian, in order", () => {
    expect(THINKING_MODES.map((m) => THINKING_LABEL[m])).toEqual(["Spento", "Automatico", "Approfondito"]);
  });
  it("only accepts the three values", () => {
    expect(["off", "auto", "deep"].every(isThinkingMode)).toBe(true);
    expect([undefined, "", "on", "Deep", 1].some(isThinkingMode)).toBe(false);
  });
  it("offers the setting only for a model with the reasoning capability", () => {
    expect(canReason({ capabilities: ["chat", "tools", "reasoning"] })).toBe(true);
    expect(canReason({ capabilities: ["chat"] })).toBe(false);
    expect(canReason(undefined)).toBe(false);
    expect(canReason(null)).toBe(false);
  });
  it("sends the choice of the session, else the profile's, else automatic", () => {
    expect(effectiveThinking("deep", "off")).toBe("deep");
    expect(effectiveThinking(undefined, "off")).toBe("off");
    expect(effectiveThinking(undefined, undefined)).toBe("auto");
  });
});
