import type { McpServer, ToolInfo, ToolRisk } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";

/** Risk of a tool as label and pill tone; the risk decides when the agent has to ask. */
export const TOOL_RISK: Record<ToolRisk, { label: string; tone: PillTone; hint: string }> = {
  read: { label: "lettura", tone: "ok", hint: "Legge, non modifica nulla" },
  write: { label: "scrittura", tone: "warn", hint: "Modifica file nel workspace" },
  exec: { label: "esecuzione", tone: "crit", hint: "Esegue comandi" },
  network: { label: "rete", tone: "info", hint: "Accede alla rete" },
  computer: { label: "computer-use", tone: "crit", hint: "Agisce su browser o desktop come farebbe una persona" },
};

export const RISK_ORDER: ToolRisk[] = ["read", "write", "network", "exec", "computer"];

export type RiskFilter = ToolRisk | "all";

export const isBrowserTool = (t: Pick<ToolInfo, "name">) => t.name.startsWith("browser_");
export const isComputerTool = (t: Pick<ToolInfo, "name">) => t.name.startsWith("computer_");

const FILE_TOOLS = new Set(["read_file", "list_dir", "glob", "grep", "write_file", "edit_file", "move_file", "delete_file", "make_dir"]);
const AGENT_TOOLS = new Set(["todo", "agent", "skill"]);

export interface ToolGroup {
  id: string;
  title: string;
  hint?: string;
  tools: ToolInfo[];
}

interface CategoryDef {
  id: string;
  title: string;
  hint: string;
}

const CATEGORIES: CategoryDef[] = [
  { id: "files", title: "File e cartelle", hint: "Leggere, cercare e modificare i file del workspace" },
  { id: "shell", title: "Shell", hint: "Comandi nella sandbox" },
  { id: "web", title: "Web", hint: "Pagine scaricate e convertite in testo" },
  { id: "browser", title: "Browser controllato", hint: "Chrome o Edge dedicato, via CDP" },
  { id: "computer", title: "Computer-use", hint: "Schermo, mouse e tastiera" },
  { id: "agent", title: "Agenti e piano", hint: "Lista dei passi, subagenti e skill" },
  { id: "other", title: "Altri tool", hint: "" },
];

/** Id of the group a tool belongs to: a built-in category, or `mcp:<server>`. */
export function toolCategory(t: ToolInfo): string {
  if (t.source === "mcp") return `mcp:${t.server ?? "?"}`;
  if (isBrowserTool(t)) return "browser";
  if (isComputerTool(t)) return "computer";
  if (FILE_TOOLS.has(t.name)) return "files";
  if (t.name === "shell") return "shell";
  if (t.name === "web_fetch") return "web";
  if (AGENT_TOOLS.has(t.name)) return "agent";
  return "other";
}

/** `mcp__github__list_issues` → `list_issues`; built-in names are unchanged. */
export function shortToolName(t: Pick<ToolInfo, "name" | "source" | "server">): string {
  if (t.source !== "mcp" || !t.server) return t.name;
  const prefix = `mcp__${t.server}__`;
  return t.name.startsWith(prefix) ? t.name.slice(prefix.length) : t.name;
}

/** Case-insensitive match on name, description, server and the title of the group. */
export function matchesTool(t: ToolInfo, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [t.name, t.description, t.server ?? "", TOOL_RISK[t.risk].label].some((s) => s.toLowerCase().includes(q));
}

/**
 * Groups tools by what they do: built-in categories first, then one group per MCP server (by name).
 * `query` and `risk` filter the tools; groups left without tools are dropped.
 */
export function groupTools(tools: ToolInfo[], opts: { query?: string; risk?: RiskFilter } = {}): ToolGroup[] {
  const { query = "", risk = "all" } = opts;
  const byId = new Map<string, ToolInfo[]>();
  for (const t of tools) {
    if (risk !== "all" && t.risk !== risk) continue;
    if (!matchesTool(t, query)) continue;
    const id = toolCategory(t);
    byId.set(id, [...(byId.get(id) ?? []), t]);
  }
  const groups: ToolGroup[] = [];
  for (const c of CATEGORIES) {
    const list = byId.get(c.id);
    if (list) groups.push({ id: c.id, title: c.title, hint: c.hint || undefined, tools: list });
  }
  const servers = [...byId.keys()].filter((id) => id.startsWith("mcp:")).sort((a, b) => a.localeCompare(b));
  for (const id of servers) groups.push({ id, title: `MCP · ${id.slice(4)}`, hint: "Server MCP", tools: byId.get(id)! });
  return groups;
}

export function countEnabled(tools: ToolInfo[]): number {
  return tools.filter((t) => t.enabled).length;
}

/** State of the group switch: every tool on, every tool off, or a mix. */
export function groupState(tools: ToolInfo[]): "all" | "none" | "some" {
  const on = countEnabled(tools);
  return on === 0 ? "none" : on === tools.length ? "all" : "some";
}

export interface ServerTool {
  /** Name as the server reported it. */
  name: string;
  /** The registry entry, when the tool list knows it (risk, description, switch). */
  info?: ToolInfo;
}

/**
 * Tools of one MCP server. The server reports its tool names (short or already prefixed); the
 * registry has the details. Registry tools of that server that the server did not list are added.
 */
export function serverTools(server: Pick<McpServer, "name" | "tools">, all: ToolInfo[]): ServerTool[] {
  const prefix = `mcp__${server.name}__`;
  const mine = all.filter((t) => t.source === "mcp" && t.server === server.name);
  const out: ServerTool[] = [];
  const used = new Set<string>();
  for (const name of server.tools) {
    const info = all.find((t) => t.name === name) ?? mine.find((t) => t.name === `${prefix}${name}`);
    if (info) used.add(info.name);
    out.push({ name: info ? shortToolName(info) : name, info });
  }
  for (const info of mine) if (!used.has(info.name)) out.push({ name: shortToolName(info), info });
  return out;
}

/** What the browser and computer-use tab shows for one capability. */
export type CapabilityStatus = "ready" | "partial" | "unavailable" | "off" | "absent";

/**
 * `absent`: the build has no such tools; `unavailable`: they exist but cannot run here (reason
 * attached); `off`: all usable ones are switched off; `partial`: some are unavailable or off.
 */
export function capabilityStatus(tools: ToolInfo[]): { status: CapabilityStatus; reason?: string } {
  if (tools.length === 0) return { status: "absent" };
  const usable = tools.filter((t) => t.available);
  const reason = tools.find((t) => !t.available && t.unavailable_reason)?.unavailable_reason;
  if (usable.length === 0) return { status: "unavailable", reason };
  if (usable.every((t) => !t.enabled)) return { status: "off" };
  if (usable.length < tools.length) return { status: "partial", reason };
  if (usable.some((t) => !t.enabled)) return { status: "partial", reason: "Alcuni tool sono disattivati" };
  return { status: "ready" };
}
