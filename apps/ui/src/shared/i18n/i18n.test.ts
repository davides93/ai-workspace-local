import { afterEach, describe, expect, it } from "vitest";
import { en } from "./en";
import { it as itCatalog } from "./it";
import { forceLocale, localeFromBrowser, resolveLocale } from "./locale";
import { formatMessage, labelMap, t, tFields, tNode } from "./t";
import { formatBytes, formatDuration, formatNumber, relTime } from "../lib/format";

afterEach(() => forceLocale("it"));

const placeholders = (s: string): string[] => {
  // parameter names and plural selectors: {name} and {n, plural, ...}
  const names = new Set<string>();
  for (const m of s.matchAll(/\{\s*([A-Za-z_]\w*)\s*(?:,|\})/g)) names.add(m[1]);
  return [...names].sort();
};

describe("catalogs", () => {
  it("en has exactly the keys of it", () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(itCatalog).sort());
  });
  it("every message is a non-empty string in both languages", () => {
    for (const [k, v] of Object.entries(itCatalog)) expect(v, `it ${k}`).toEqual(expect.any(String));
    for (const [k, v] of Object.entries(en)) expect(v.trim(), `en ${k}`).not.toBe("");
  });
  it("placeholders match per key", () => {
    const bad: string[] = [];
    for (const k of Object.keys(itCatalog) as (keyof typeof itCatalog)[]) {
      const a = placeholders(itCatalog[k]).join(",");
      const b = placeholders(en[k]).join(",");
      if (a !== b) bad.push(`${k}: it {${a}} vs en {${b}}`);
    }
    expect(bad).toEqual([]);
  });
});

describe("language resolution", () => {
  it("auto: Italian when the browser language starts with it, English otherwise", () => {
    expect(localeFromBrowser(["it-IT", "en"])).toBe("it");
    expect(localeFromBrowser("it")).toBe("it");
    expect(localeFromBrowser(["en-US"])).toBe("en");
    expect(localeFromBrowser(["fr-FR", "it"])).toBe("en");
    expect(localeFromBrowser(undefined)).toBe("en");
    expect(resolveLocale("auto", ["it-CH"])).toBe("it");
    expect(resolveLocale("en", ["it-IT"])).toBe("en");
    expect(resolveLocale("it", ["en-US"])).toBe("it");
  });
  it("forcing a locale sets <html lang> when there is a document", () => {
    forceLocale("en");
    expect(t("common.language")).toBe("Language");
    forceLocale("it");
    expect(t("common.language")).toBe("Lingua");
  });
});

describe("t()", () => {
  it("interpolates parameters", () => {
    expect(formatMessage("Ciao {name}, {n} file", { name: "Anna", n: 3 }, "it")).toBe("Ciao Anna, 3 file");
  });
  it("keeps an unknown placeholder visible instead of failing", () => {
    expect(formatMessage("Ciao {name}", {}, "it")).toBe("Ciao {name}");
  });
  it("plurals follow Intl.PluralRules of each language", () => {
    const m = "{n, plural, =0 {nessuno} one {# elemento} other {# elementi}}";
    expect(formatMessage(m, { n: 0 }, "it")).toBe("nessuno");
    expect(formatMessage(m, { n: 1 }, "it")).toBe("1 elemento");
    expect(formatMessage(m, { n: 2 }, "it")).toBe("2 elementi");
    const e = "{n, plural, one {# file} other {# files}}";
    expect(formatMessage(e, { n: 1 }, "en")).toBe("1 file");
    expect(formatMessage(e, { n: 5 }, "en")).toBe("5 files");
    expect(formatMessage(e, { n: 1234 }, "en")).toBe("1,234 files");
  });
  it("plural branches can hold parameters", () => {
    expect(formatMessage("{n, plural, one {{who} ha # voto} other {{who} ha # voti}}", { n: 2, who: "Gino" }, "it")).toBe("Gino ha 2 voti");
  });
  it("labelMap follows the language at every read", () => {
    const labels = labelMap({ a: "common.language", b: "common.language_auto_short" });
    expect(labels.a).toBe("Lingua");
    forceLocale("en");
    expect(labels.a).toBe("Language");
    expect(labels.b).toBe("Automatic");
  });
  it("tFields keeps plain fields and follows the language for the translated ones", () => {
    const meta = tFields({ tone: "ok" }, { label: "common.language" });
    expect(meta).toEqual({ tone: "ok", label: "Lingua" });
    forceLocale("en");
    expect(meta.label).toBe("Language");
    expect(meta.tone).toBe("ok");
  });
  it("tNode puts nodes where the message says", () => {
    const node = tNode("shell.pair_help", { devices: "NODE" }) as { props: { children: unknown[] } };
    expect(node.props.children[0]).toBe("Sul dispositivo che esegue AI Workspace apri ");
    expect((node.props.children[1] as { props: { children: unknown } }).props.children).toBe("NODE");
    forceLocale("en");
    const en = tNode("shell.pair_help", { devices: "NODE" }) as { props: { children: unknown[] } };
    expect(en.props.children[0]).toBe("On the device that runs AI Workspace open ");
  });
});

describe("formatting follows the active language", () => {
  it("numbers, sizes, durations and relative times", () => {
    expect(formatNumber(12345.5)).toBe("12.345,5");
    expect(formatBytes(1536)).toBe("1,5 KB");
    expect(relTime(Date.now() - 5000)).toBe("ora");
    expect(formatDuration(3 * 86400)).toBe("3 gg");
    forceLocale("en");
    expect(formatNumber(12345.5)).toBe("12,345.5");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(relTime(Date.now() - 5000)).toBe("now");
    expect(relTime(Date.now() - 5 * 60_000)).toMatch(/5 min/);
    expect(formatDuration(3 * 86400)).toBe("3 d");
  });
});
