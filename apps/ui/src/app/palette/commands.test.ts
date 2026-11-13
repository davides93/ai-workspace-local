import { describe, expect, it } from "vitest";
import { matchesHotkey } from "../../shared/lib/useHotkey";
import { normalizeText } from "../../shared/lib/text";
import { filterCommands, type Command } from "./commands";

const cmd = (id: string, label: string, keywords: string[] = [], group: Command["group"] = "go"): Command => ({ id, label, group, icon: "plus", keywords, run: () => undefined });
const commands = [
  cmd("home", "Panoramica", ["home", "dashboard"]),
  cmd("act", "Attività", ["audit", "log"]),
  cmd("models", "Modelli", ["scarica", "download"]),
  cmd("dl", "Scarica un modello", ["hugging face"], "actions"),
  cmd("kill", "Kill switch", ["stop", "emergenza"], "actions"),
];

describe("filterCommands", () => {
  it("returns everything for an empty query", () => {
    expect(filterCommands(commands, "  ")).toHaveLength(commands.length);
  });
  it("ignores accents and case", () => {
    expect(normalizeText("Attività")).toBe("attivita");
    expect(filterCommands(commands, "ATTIVITA").map((c) => c.id)).toEqual(["act"]);
  });
  it("matches keywords", () => {
    expect(filterCommands(commands, "audit").map((c) => c.id)).toEqual(["act"]);
    expect(filterCommands(commands, "emergenza").map((c) => c.id)).toEqual(["kill"]);
  });
  it("requires every word to match", () => {
    expect(filterCommands(commands, "scarica modello").map((c) => c.id)).toEqual(["dl"]);
    expect(filterCommands(commands, "scarica xyz")).toEqual([]);
  });
  it("ranks label matches before keyword matches", () => {
    expect(filterCommands(commands, "scarica").map((c) => c.id)).toEqual(["dl", "models"]);
  });
});

describe("matchesHotkey", () => {
  const ev = (init: Partial<KeyboardEvent>) => ({ key: "k", ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...init });
  it("accepts Ctrl or Cmd for mod", () => {
    expect(matchesHotkey(ev({ ctrlKey: true }), { key: "k", mod: true })).toBe(true);
    expect(matchesHotkey(ev({ metaKey: true }), { key: "k", mod: true })).toBe(true);
    expect(matchesHotkey(ev({ key: "K", ctrlKey: true }), { key: "k", mod: true })).toBe(true);
  });
  it("rejects other combinations", () => {
    expect(matchesHotkey(ev({}), { key: "k", mod: true })).toBe(false);
    expect(matchesHotkey(ev({ ctrlKey: true, shiftKey: true }), { key: "k", mod: true })).toBe(false);
    expect(matchesHotkey(ev({ ctrlKey: true, altKey: true }), { key: "k", mod: true })).toBe(false);
    expect(matchesHotkey(ev({ key: "j", ctrlKey: true }), { key: "k", mod: true })).toBe(false);
  });
});
