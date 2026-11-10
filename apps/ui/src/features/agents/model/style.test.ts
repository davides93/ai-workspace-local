import { describe, expect, it } from "vitest";
import { STYLE_PRESETS, styleOf, withStyle } from "./style";

describe("style presets", () => {
  it("map Preciso, Bilanciato and Creativo to temperature and top-p", () => {
    expect(STYLE_PRESETS.map((p) => [p.label, p.temperature, p.top_p])).toEqual([["Preciso", 0.2, 0.9], ["Bilanciato", 0.6, 0.95], ["Creativo", 0.9, 0.95]]);
  });
  it("recognise a preset only when both numbers match", () => {
    expect(styleOf({ temperature: 0.2, top_p: 0.9 })).toBe("precise");
    expect(styleOf({ temperature: 0.6, top_p: 0.95 })).toBe("balanced");
    expect(styleOf({ temperature: 0.9, top_p: 0.95 })).toBe("creative");
    expect(styleOf({ temperature: 0.2 })).toBe("custom");
    expect(styleOf({ temperature: 0.2, top_p: 0.95 })).toBe("custom");
    expect(styleOf({})).toBe("default");
  });
  it("set both numbers and touch nothing else", () => {
    const params = { temperature: 1.3, top_p: 0.5, context: 8192, max_tokens: 2048, thinking: "auto" as const, adapters: [{ path: "/a.gguf" }] };
    expect(withStyle(params, "precise")).toEqual({ ...params, temperature: 0.2, top_p: 0.9 });
    expect(withStyle(params, "creative")).toMatchObject({ temperature: 0.9, top_p: 0.95, context: 8192, max_tokens: 2048, adapters: [{ path: "/a.gguf" }] });
  });
  it("forget the numbers for the model's default, and leave a custom mix alone", () => {
    const params = { temperature: 0.2, top_p: 0.9, context: 4096 };
    expect(withStyle(params, "default")).toEqual({ context: 4096 });
    expect(withStyle(params, "custom")).toBe(params);
  });
});
