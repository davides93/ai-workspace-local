import { describe, expect, it } from "vitest";
import { matchesHotkey } from "../../shared/lib/useHotkey";
import { normalizeText } from "../../shared/lib/text";
import type { AgentProfile } from "../../shared/api/types";
import { filterCommands, type Command } from "./commands";
import { buildCommands, type CommandDeps } from "./useCommands";

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

describe("buildCommands", () => {
  const agent = (id: string): AgentProfile => ({ id, name: id, description: "", inference_profile: "auto:agent", system_prompt: "", tools: [], mode: "ask", sandbox: "workspace-write", max_turns: 10 });
  const calls: string[] = [];
  const deps = (over: Partial<CommandDeps> = {}): CommandDeps => ({
    mode: "simple", agents: [agent("operator"), agent("coder"), agent("assistant")], configuredAgent: "coder",
    newSession: (id) => calls.push(`new:${id}`), refreshRuntimes: () => calls.push("refresh"), kill: () => calls.push("kill"),
    setMode: (m) => calls.push(`mode:${m}`), go: (to) => calls.push(`go:${to}`), askInChat: (text) => calls.push(`ask:${text}`), ...over,
  });
  const run = (list: Command[], id: string) => { calls.length = 0; list.find((c) => c.id === id)!.run(); return [...calls]; };

  it("simple: «Vai a» lists the chat only", () => {
    expect(buildCommands(deps()).filter((c) => c.group === "go").map((c) => c.id)).toEqual(["go-chat"]);
  });
  it("developer: «Vai a» lists every one of the 19 sections", () => {
    expect(buildCommands(deps({ mode: "developer" })).filter((c) => c.group === "go")).toHaveLength(19);
  });
  it("simple: «Nuova sessione» starts one with the Assistente, whatever the configured default is", () => {
    expect(run(buildCommands(deps()), "new-session")).toEqual(["new:assistant"]);
  });
  it("simple: falls back to the configured default when the server has no Assistente, and to the chat when there are no agents yet", () => {
    expect(run(buildCommands(deps({ agents: [agent("operator"), agent("coder")] })), "new-session")).toEqual(["new:coder"]);
    expect(run(buildCommands(deps({ agents: undefined })), "new-session")).toEqual(["go:/chat"]);
  });
  it("developer: «Nuova sessione» still starts one with the operator", () => {
    expect(run(buildCommands(deps({ mode: "developer" })), "new-session")).toEqual(["new:operator"]);
    expect(run(buildCommands(deps({ mode: "developer", agents: undefined })), "new-session")).toEqual(["go:/agents"]);
  });
  it("simple: «Scarica un modello» opens the chat with the request in the message box, not the Modelli page", () => {
    expect(run(buildCommands(deps()), "download-model")).toEqual(["ask:Scarica un modello adatto a questo computer"]);
  });
  it("developer: «Scarica un modello» still goes to the catalog", () => {
    expect(run(buildCommands(deps({ mode: "developer" })), "download-model")).toEqual(["go:/models/discover"]);
  });
  it("keeps the kill switch, the theme and the way to the other mode in both modes", () => {
    const simple = buildCommands(deps()).map((c) => c.id);
    expect(simple).toEqual(expect.arrayContaining(["kill", "theme-light", "theme-dark", "theme-system", "mode-developer"]));
    expect(simple).not.toContain("detect-runtimes");
    const developer = buildCommands(deps({ mode: "developer" })).map((c) => c.id);
    expect(developer).toEqual(expect.arrayContaining(["kill", "theme-light", "mode-simple", "detect-runtimes"]));
    expect(run(buildCommands(deps()), "mode-developer")).toEqual(["mode:developer"]);
    expect(run(buildCommands(deps()), "kill")).toEqual(["kill"]);
  });
});
