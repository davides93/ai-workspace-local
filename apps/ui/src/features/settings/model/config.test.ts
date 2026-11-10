import { describe, expect, it } from "vitest";
import type { ConfigView, SettingsSchema } from "../../../shared/api/types";
import {
  configTheme, configUiMode, definingLayers, formatValue, groupSettings, hasOwn, hintOf, overriddenBy, parseNumber, sectionOf, shortTitle, shownValue, uiModeLayer, winningLayer,
} from "./config";

const schema: SettingsSchema = {
  type: "object",
  properties: {
    "ui.theme": { type: "string", title: "Tema", enum: ["system", "light", "dark"], default: "system" },
    "policy.mode": { type: "string", title: "Modalità di approvazione", default: "ask" },
    "network.allow_domains": { type: "array", title: "Domini consentiti", default: [] },
    "server.port": { type: "integer", title: "Porta API", default: 7777, minimum: 1024, maximum: 65535 },
    "models.dir": { type: "string", title: "Cartella modelli", default: "" },
    "x.unknown": { type: "string", title: "Sconosciuta" },
  },
};

const view: ConfigView = {
  schema,
  layers: [
    { name: "defaults", values: { "ui.theme": "system", "policy.mode": "ask", "network.allow_domains": [], "server.port": 7777 } },
    { name: "global", path: "~/.aiw/config.json", values: { "ui.theme": "dark", "policy.mode": "ask", "network.allow_domains": ["docs.rs"] } },
    { name: "device", path: "~/.aiw/devices/pc.json", values: { "policy.mode": "plan" } },
    { name: "workspace", values: { "policy.mode": "auto-edit" } },
  ],
  effective: { "ui.theme": "dark", "policy.mode": "auto-edit", "network.allow_domains": ["docs.rs"], "server.port": 7777 },
};

describe("layers", () => {
  it("finds the layer that wins: the highest one that defines the key", () => {
    expect(winningLayer(view, "policy.mode")).toBe("workspace");
    expect(winningLayer(view, "ui.theme")).toBe("global");
    expect(winningLayer(view, "server.port")).toBe("defaults");
    expect(winningLayer(view, "never.set")).toBe("defaults");
  });
  it("lists the layers that define a key, lowest first, ignoring the defaults layer", () => {
    expect(definingLayers(view, "policy.mode")).toEqual(["global", "device", "workspace"]);
    expect(definingLayers(view, "server.port")).toEqual([]);
    expect(hasOwn(view, "device", "ui.theme")).toBe(false);
  });
  it("tells when a value written in a layer would be hidden by a higher one", () => {
    expect(overriddenBy(view, "global", "policy.mode")).toBe("workspace");
    expect(overriddenBy(view, "workspace", "policy.mode")).toBeNull();
    expect(overriddenBy(view, "global", "ui.theme")).toBeNull();
    expect(overriddenBy(view, "device", "ui.theme")).toBeNull();
  });
  it("shows the layer's own value, else what it inherits", () => {
    const f = (k: string) => schema.properties[k];
    expect(shownValue(view, "device", "policy.mode", f("policy.mode"))).toBe("plan");
    expect(shownValue(view, "device", "ui.theme", f("ui.theme"))).toBe("dark");
    expect(shownValue(view, "workspace", "server.port", f("server.port"))).toBe(7777);
    expect(shownValue(view, "global", "models.dir", f("models.dir"))).toBe("");
  });
  it("treats an own empty list as a value (it overrides)", () => {
    const v: ConfigView = { ...view, layers: view.layers.map((l) => (l.name === "device" ? { ...l, values: { "network.allow_domains": [] } } : l)) };
    expect(winningLayer(v, "network.allow_domains")).toBe("device");
  });
});

describe("sections", () => {
  it("files a key under the section of its prefix", () => {
    expect(sectionOf("ui.theme").id).toBe("ui");
    expect(sectionOf("network.allow_domains").id).toBe("security");
    expect(sectionOf("workspace.root").id).toBe("folders");
    expect(sectionOf("x.unknown").id).toBe("other");
  });
  it("puts the voice settings in their own section", () => {
    const voice = { type: "object", properties: { "voice.language": { type: "string" }, "voice.stt_runtime": { type: "string" }, "server.port": { type: "integer" } } } as unknown as SettingsSchema;
    const groups = groupSettings(voice);
    expect(groups.map((g) => g.section.id)).toEqual(["server", "voice"]);
    expect(groups[1].entries.map((e) => e.key)).toEqual(["voice.language", "voice.stt_runtime"]);
    expect(sectionOf("voice.tts_voice").title).toBe("Voce");
  });

  it("groups the schema in the fixed order and drops empty sections", () => {
    expect(groupSettings(schema).map((g) => g.section.id)).toEqual(["ui", "security", "server", "folders", "other"]);
    expect(groupSettings(schema).find((g) => g.section.id === "security")!.entries.map((e) => e.key)).toEqual(["policy.mode", "network.allow_domains"]);
  });
  it("filters by key, title and hint", () => {
    expect(groupSettings(schema, "porta").flatMap((g) => g.entries.map((e) => e.key))).toEqual(["server.port"]);
    expect(groupSettings(schema, "NETWORK").flatMap((g) => g.entries.map((e) => e.key))).toEqual(["network.allow_domains"]);
    expect(groupSettings(schema, "web_fetch").flatMap((g) => g.entries.map((e) => e.key))).toEqual(["network.allow_domains"]);
    expect(groupSettings(schema, "zzz")).toEqual([]);
  });
  it("prefers the description of the schema to the written hint", () => {
    expect(hintOf("ui.theme", { type: "string", description: "Mia" })).toBe("Mia");
    expect(hintOf("ui.theme", { type: "string" })).toMatch(/selettore/);
    expect(hintOf("x.unknown", { type: "string" })).toBeUndefined();
  });
});

describe("parseNumber", () => {
  const port = { type: "integer" as const, minimum: 1024, maximum: 65535 };
  it("reads a number inside its bounds", () => {
    expect(parseNumber("7777", port)).toEqual({ ok: true, value: 7777 });
    expect(parseNumber(" 0,85 ", { type: "number", minimum: 0.3, maximum: 0.95 })).toEqual({ ok: true, value: 0.85 });
  });
  it("explains what is wrong", () => {
    expect(parseNumber("", port)).toMatchObject({ ok: false });
    expect(parseNumber("abc", port)).toMatchObject({ ok: false, error: "Non è un numero" });
    expect(parseNumber("1.5", port)).toMatchObject({ ok: false, error: "Serve un numero intero" });
    expect(parseNumber("80", port)).toMatchObject({ ok: false, error: "Il minimo è 1024" });
    expect(parseNumber("70000", port)).toMatchObject({ ok: false, error: "Il massimo è 65535" });
  });
});

describe("values", () => {
  it("drops the remark in brackets from a title", () => {
    expect(shortTitle("Domini consentiti a web/browser (vuoto = tutti i pubblici)")).toBe("Domini consentiti a web/browser");
    expect(shortTitle("Sandbox")).toBe("Sandbox");
    expect(shortTitle("(solo note)")).toBe("(solo note)");
  });
  it("formats values for the summary", () => {
    expect(formatValue(["a", "b"])).toBe("a, b");
    expect(formatValue([])).toBe("vuoto");
    expect(formatValue("")).toBe("vuoto");
    expect(formatValue(true)).toBe("sì");
    expect(formatValue(7777)).toBe("7777");
  });
  it("reads the theme the configuration asks for, only when valid", () => {
    expect(configTheme(view)).toBe("dark");
    expect(configTheme({ ...view, effective: { "ui.theme": "pink" } })).toBeUndefined();
    expect(configTheme(undefined)).toBeUndefined();
  });
  it("reads the interface mode the configuration asks for, only when valid", () => {
    expect(configUiMode({ ...view, effective: { "ui.mode": "developer" } })).toBe("developer");
    expect(configUiMode({ ...view, effective: { "ui.mode": "simple" } })).toBe("simple");
    expect(configUiMode({ ...view, effective: { "ui.mode": "expert" } })).toBeUndefined();
    expect(configUiMode(view)).toBeUndefined();
    expect(configUiMode(undefined)).toBeUndefined();
  });
  it("writes the interface mode to the layer that wins today, else to the global one", () => {
    expect(uiModeLayer(undefined)).toBe("global");
    expect(uiModeLayer(view)).toBe("global");
    const inDevice = { ...view, layers: view.layers.map((l) => (l.name === "device" ? { ...l, values: { "ui.mode": "developer" } } : l)) };
    expect(uiModeLayer(inDevice)).toBe("device");
  });
});
