// Tool selection of an agent (AgentProfile.tools). Pure logic behind the ToolPicker.
//
// Rules of the backend this mirrors (crates/aiw-agent/src/runner.rs):
//  - `tools` is a list of names, and `"*"` means every tool that is enabled;
//  - seven tools are internal to the agent loop (todo, agent, skill, render_ui, code, handoff, team_state): the catalogue
//    (`GET /api/tools`) lists them with `source: "internal"` and the backend accepts them by name, or by `"*"`,
//    like any other. Dropping them while editing a list would silently take the plan, sub-agents, skills and
//    code mode away from the agent. `INTERNAL_TOOLS` is the fallback for a catalogue that does not list them
//    (an older backend): the catalogue's own entry wins, so there is never a duplicate.

import type { PolicyMode, SandboxMode, ToolInfo, ToolRisk } from "../../../shared/api/types";

export const ALL = "*";

export interface InternalTool {
  name: string;
  description: string;
  risk: ToolRisk;
}

export const INTERNAL_TOOLS: InternalTool[] = [
  { name: "todo", risk: "read", description: "Pubblica e aggiorna il piano di lavoro: la lista dei passi che vedi nel pannello del run." },
  { name: "agent", risk: "read", description: "Delega un sotto-compito a un altro agente, con un contesto separato." },
  { name: "skill", risk: "read", description: "Carica le istruzioni di una skill elencata nel prompt di sistema." },
  { name: "render_ui", risk: "read", description: "Mostra un'interfaccia interattiva (A2UI) nella chat: moduli, scelte, riepiloghi." },
  { name: "code", risk: "read", description: "Orchestra più tool con uno script in un solo passo. Ogni chiamata dello script passa da policy, conferme e hook come una diretta." },
  { name: "handoff", risk: "read", description: "Passa la conversazione a un altro agente: il tuo run termina e prosegue lui con il tuo riassunto." },
  { name: "team_state", risk: "read", description: "Legge e scrive lo stato condiviso tra gli agenti di un team: serve solo dentro un team." },
];

export const INTERNAL_NAMES: ReadonlySet<string> = new Set(INTERNAL_TOOLS.map((t) => t.name));

/** An internal tool of the agent loop: by the catalogue's `source`, or by name for the ones that are known. */
const isInternal = (t: Pick<ToolInfo, "name"> & Partial<Pick<ToolInfo, "source">>): boolean => t.source === "internal" || INTERNAL_NAMES.has(t.name);

export type CategoryId = "file" | "shell" | "web" | "browser" | "computer" | "internal" | "other" | `mcp:${string}`;

const FILE_TOOLS = new Set(["read_file", "write_file", "edit_file", "move_file", "list_dir", "glob", "grep"]);

/** One entry of the picker: a catalogue tool or an internal one, already classified. */
export interface PickerTool {
  name: string;
  description: string;
  risk: ToolRisk;
  category: CategoryId;
  /** Turned off for everyone in "Tool, MCP e browser". */
  disabled: boolean;
  /** Cannot run on this device (reason in `unavailable`). */
  unavailable?: string;
}

function mcpServer(t: Pick<ToolInfo, "name" | "server">): string {
  if (t.server) return t.server;
  const m = /^mcp__(.+?)__/.exec(t.name);
  return m ? m[1] : "mcp";
}

export function categoryOf(t: Pick<ToolInfo, "name" | "source" | "server">): CategoryId {
  if (isInternal(t)) return "internal";
  if (t.source === "mcp" || t.name.startsWith("mcp__")) return `mcp:${mcpServer(t)}`;
  if (t.name.startsWith("browser_")) return "browser";
  if (t.name.startsWith("computer_")) return "computer";
  if (FILE_TOOLS.has(t.name)) return "file";
  if (t.name === "shell") return "shell";
  if (t.name.startsWith("web_")) return "web";
  return "other";
}

export const CATEGORY_LABEL: Record<Exclude<CategoryId, `mcp:${string}`>, string> = {
  file: "File e cartelle",
  shell: "Shell",
  web: "Web",
  browser: "Browser",
  computer: "Computer-use",
  internal: "Interni",
  other: "Altri tool",
};

export const categoryLabel = (c: CategoryId): string => (c.startsWith("mcp:") ? `Server MCP · ${c.slice(4)}` : CATEGORY_LABEL[c as keyof typeof CATEGORY_LABEL]);

const ORDER: CategoryId[] = ["file", "shell", "web", "browser", "computer"];
const rank = (c: CategoryId): number => {
  const i = ORDER.indexOf(c);
  if (i >= 0) return i;
  if (c.startsWith("mcp:")) return ORDER.length;
  if (c === "internal") return ORDER.length + 1;
  return ORDER.length + 2;
};

/**
 * Catalogue tools, plus the internal tools the catalogue did not list (the built-in descriptions fill in for an
 * older backend). An internal tool is never "disattivato": it has no switch.
 */
export function buildCatalog(tools: readonly ToolInfo[]): PickerTool[] {
  const out: PickerTool[] = tools.map((t) => ({
    name: t.name, description: t.description, risk: t.risk, category: categoryOf(t),
    disabled: isInternal(t) ? false : !t.enabled, unavailable: t.available ? undefined : t.unavailable_reason || "non disponibile",
  }));
  const listed = new Set(out.map((t) => t.name));
  for (const t of INTERNAL_TOOLS) {
    if (!listed.has(t.name)) out.push({ name: t.name, description: t.description, risk: t.risk, category: "internal", disabled: false });
  }
  return out;
}

export interface ToolGroup {
  id: CategoryId;
  label: string;
  tools: PickerTool[];
}

const matches = (t: PickerTool, q: string): boolean =>
  !q || t.name.toLowerCase().includes(q) || t.description.toLowerCase().includes(q) || categoryLabel(t.category).toLowerCase().includes(q);

/** Tools grouped by category in a fixed order; `query` filters by name, description or group. */
export function groupTools(catalog: readonly PickerTool[], query = ""): ToolGroup[] {
  const q = query.trim().toLowerCase();
  const groups = new Map<CategoryId, PickerTool[]>();
  for (const t of catalog) {
    if (!matches(t, q)) continue;
    groups.set(t.category, [...(groups.get(t.category) ?? []), t]);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => rank(a) - rank(b) || a.localeCompare(b))
    .map(([id, tools]) => ({ id, label: categoryLabel(id), tools }));
}

export const TRAINING_TOOL = "training_harness";

/** A wildcard subsumes ordinary tools; training always needs an explicit grant. */
export function normalizeTools(tools: readonly string[]): string[] {
  if (tools.includes(ALL)) return tools.includes(TRAINING_TOOL) ? [ALL, TRAINING_TOOL] : [ALL];
  return [...new Set(tools.map((t) => t.trim()).filter(Boolean))];
}

export const isAllTools = (tools: readonly string[]): boolean => tools.includes(ALL);

export const isSelected = (tools: readonly string[], name: string): boolean => tools.includes(name) || (name !== TRAINING_TOOL && tools.includes(ALL));

/** Turns `"*"` into the explicit list of every tool of the catalogue (internal ones included). */
export function expandAll(catalog: readonly PickerTool[]): string[] {
  return catalog.map((t) => t.name);
}

/** Switches "all tools" on (`["*"]`) or off (every tool explicitly, so nothing is lost). */
export function setAllTools(tools: readonly string[], all: boolean, catalog: readonly PickerTool[]): string[] {
  if (all) return [ALL];
  return tools.includes(ALL) ? expandAll(catalog) : [...tools];
}

export function toggleTool(tools: readonly string[], name: string, on: boolean): string[] {
  const rest = tools.filter((t) => t !== name);
  return on ? [...rest, name] : rest;
}

/** Adds or removes every tool of a group at once. */
export function setGroup(tools: readonly string[], names: readonly string[], on: boolean): string[] {
  const set = new Set(names);
  const rest = tools.filter((t) => !set.has(t));
  return on ? [...rest, ...names] : rest;
}

export type GroupState = "all" | "some" | "none";

export function groupState(tools: readonly string[], names: readonly string[]): GroupState {
  if (names.length === 0) return "none";
  const n = names.filter((x) => isSelected(tools, x)).length;
  return n === 0 ? "none" : n === names.length ? "all" : "some";
}

/** Names in the list that the catalogue does not know (an MCP server that is gone, a renamed tool). */
export function orphanTools(tools: readonly string[], catalog: readonly PickerTool[]): string[] {
  const known = new Set(catalog.map((t) => t.name));
  return tools.filter((t) => t !== ALL && !known.has(t));
}

export interface ToolSummary {
  count: number;
  total: number;
  byRisk: Partial<Record<ToolRisk, number>>;
}

export function summarizeTools(tools: readonly string[], catalog: readonly PickerTool[]): ToolSummary {
  const picked = catalog.filter((t) => isSelected(tools, t.name));
  const byRisk: Partial<Record<ToolRisk, number>> = {};
  for (const t of picked) byRisk[t.risk] = (byRisk[t.risk] ?? 0) + 1;
  return { count: picked.length, total: catalog.length, byRisk };
}

/** Selected tools the agent could never use because of its own mode or sandbox. */
export function blockedTools(tools: readonly string[], catalog: readonly PickerTool[], mode: PolicyMode, sandbox: SandboxMode): { name: string; why: string }[] {
  const out: { name: string; why: string }[] = [];
  for (const t of catalog) {
    if (!isSelected(tools, t.name)) continue;
    if (mode === "plan" && (t.risk === "write" || t.risk === "exec" || t.risk === "computer")) {
      out.push({ name: t.name, why: "la modalità Piano nega scritture, comandi e azioni sul computer" });
    } else if (sandbox === "read-only" && (t.name === "write_file" || t.name === "edit_file" || t.name === "move_file")) {
      out.push({ name: t.name, why: "la sandbox in sola lettura non consente di scrivere" });
    }
  }
  return out;
}
