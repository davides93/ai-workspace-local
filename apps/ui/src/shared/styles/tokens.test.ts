import { describe, expect, it } from "vitest";
import css from "./tokens.css?raw";
import oled from "./oled.css?raw";

// WCAG 2.1 contrast of the design tokens, computed from tokens.css itself.

function block(selector: string): Record<string, string> {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`blocco ${selector} non trovato`);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("\n}", start));
  const vars: Record<string, string> = {};
  for (const m of body.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) vars[m[1]] = m[2];
  return vars;
}

const luminance = (hex: string) => {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [n >> 16, (n >> 8) & 255, n & 255].map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a: string, b: string) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

const themes = { chiaro: block(":root"), scuro: block(':root[data-theme="dark"]') };
// On touch screens the dark theme uses true black (oled.css): the code block gets darker, the syntax colors must still read.
const oledCodeBg = /--code-bg:\s*(#[0-9a-fA-F]{6})/.exec(oled)?.[1] ?? "";

describe("tokens.css", () => {
  it("has a single dark block keyed on data-theme", () => {
    expect(css.match(/data-theme="dark"\]\s*\{/g)?.length).toBe(1);
    expect(css).not.toMatch(/prefers-color-scheme:\s*dark\)\s*\{\s*:root:not/);
  });
  it("defines the spacing, z-index and duration scales", () => {
    for (const v of ["--space-1", "--space-8", "--z-overlay", "--z-toast", "--dur-fast", "--dur-slow"]) expect(css).toContain(v);
  });
  for (const [name, t] of Object.entries(themes)) {
    describe(`tema ${name}`, () => {
      it("defines every color token used for text and surfaces", () => {
        for (const k of ["bg", "surface", "surface-2", "fg", "fg-muted", "fg-faint", "ok", "warn", "crit", "info"]) expect(t[k], k).toBeDefined();
      });
      it("keeps muted and faint text at >= 4.5:1 on every surface", () => {
        for (const fg of ["fg", "fg-muted", "fg-faint"]) {
          for (const bg of ["bg", "surface", "surface-2", "accent-soft"]) {
            expect(contrast(t[fg], t[bg]), `${fg} su ${bg}`).toBeGreaterThanOrEqual(4.5);
          }
        }
      });
      it("keeps status colors at >= 4.5:1 on their soft background", () => {
        for (const s of ["ok", "warn", "crit", "info"]) expect(contrast(t[s], t[`${s}-soft`]), `${s} su ${s}-soft`).toBeGreaterThanOrEqual(4.5);
        expect(contrast(t["accent-ink"], t["accent-soft"])).toBeGreaterThanOrEqual(4.5);
      });
      it("keeps status colors readable on the surface", () => {
        for (const s of ["ok", "warn", "crit", "info", "accent-ink"]) expect(contrast(t[s], t.surface), `${s} su surface`).toBeGreaterThanOrEqual(4.5);
      });
      it("keeps the syntax colors of the code blocks at >= 4.5:1 on the code background and its header", () => {
        const kinds = ["com", "str", "num", "kw", "lit", "fn", "ty", "attr", "ins", "del"];
        const backgrounds = [t["code-bg"], t["surface-2"], ...(name === "scuro" ? [oledCodeBg] : [])];
        expect(backgrounds.every((b) => /^#[0-9a-f]{6}$/i.test(b))).toBe(true);
        for (const k of kinds) {
          expect(t[`tok-${k}`], `tok-${k}`).toBeDefined();
          for (const bg of backgrounds) expect(contrast(t[`tok-${k}`], bg), `tok-${k} su ${bg}`).toBeGreaterThanOrEqual(4.5);
        }
      });
      it("keeps button text readable", () => {
        expect(contrast(t["on-accent"], t.accent)).toBeGreaterThanOrEqual(4.5);
        expect(contrast(t["on-crit"], t.crit)).toBeGreaterThanOrEqual(4.5);
      });
    });
  }
});
