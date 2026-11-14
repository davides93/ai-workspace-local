import type { McpServer, ToolInfo, ToolRisk } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";

import { t as tr, t } from "../../../shared/i18n";
/** Risk of a tool as label and pill tone; the risk decides when the agent has to ask. */
export const TOOL_RISK: Record<ToolRisk, { label: string; tone: PillTone; hint: string }> = {
  read: { get label() { return t("capabilities.lettura"); }, tone: "ok", get hint() { return t("capabilities.legge_non_modifica_nulla"); } },
  write: { get label() { return t("capabilities.scrittura"); }, tone: "warn", get hint() { return t("capabilities.modifica_file_nel_workspace"); } },
  exec: { get label() { return t("capabilities.esecuzione"); }, tone: "crit", get hint() { return t("capabilities.esegue_comandi"); } },
  network: { get label() { return t("capabilities.rete"); }, tone: "info", get hint() { return t("capabilities.accede_alla_rete"); } },
  computer: { get label() { return t("capabilities.computer_use_2"); }, tone: "crit", get hint() { return t("capabilities.agisce_su_browser_o_desktop"); } },
};

export const RISK_ORDER: ToolRisk[] = ["read", "write", "network", "exec", "computer"];

export type RiskFilter = ToolRisk | "all";

export const isBrowserTool = (t: Pick<ToolInfo, "name">) => t.name.startsWith("browser_");
export const isComputerTool = (t: Pick<ToolInfo, "name">) => t.name.startsWith("computer_");

const FILE_TOOLS = new Set(["read_file", "list_dir", "glob", "grep", "write_file", "edit_file", "move_file", "copy_file", "delete_file", "make_dir"]);

/**
 * Implemented by the agent loop itself (todo, agent, skill, render_ui, code): always on, listed so that an agent
 * can name them, and with no switch (the backend answers 400 to `PUT /api/tools/{name}`).
 */
export const isInternalTool = (t: Pick<ToolInfo, "source">): boolean => t.source === "internal";

/** The tools that have an on/off switch. */
export const switchableTools = (tools: ToolInfo[]): ToolInfo[] => tools.filter((t) => !isInternalTool(t));

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
  { id: "files", get title() { return t("capabilities.file_e_cartelle"); }, get hint() { return t("capabilities.leggere_cercare_e_modificare_i"); } },
  { id: "shell", get title() { return t("capabilities.shell"); }, get hint() { return t("capabilities.comandi_nella_sandbox"); } },
  { id: "web", get title() { return t("capabilities.web"); }, get hint() { return t("capabilities.pagine_scaricate_e_convertite_in"); } },
  { id: "browser", get title() { return t("capabilities.browser_controllato"); }, get hint() { return t("capabilities.chrome_o_edge_dedicato_via"); } },
  { id: "computer", get title() { return t("capabilities.computer_use"); }, get hint() { return t("capabilities.schermo_mouse_e_tastiera"); } },
  { id: "internal", get title() { return t("capabilities.interni"); }, get hint() { return t("capabilities.del_ciclo_dell_agente_piano"); } },
  { id: "other", get title() { return t("capabilities.altri_tool"); }, hint: "" },
];

/** Id of the group a tool belongs to: a built-in category, or `mcp:<server>`. */
export function toolCategory(t: ToolInfo): string {
  if (isInternalTool(t)) return "internal";
  if (t.source === "mcp") return `mcp:${t.server ?? "?"}`;
  if (isBrowserTool(t)) return "browser";
  if (isComputerTool(t)) return "computer";
  if (FILE_TOOLS.has(t.name)) return "files";
  if (t.name === "shell") return "shell";
  if (t.name === "web_fetch" || t.name === "web_search") return "web";
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
  for (const id of servers) groups.push({ id, title: tr("capabilities.mcp", { server: id.slice(4) }), hint: tr("capabilities.server_mcp"), tools: byId.get(id)! });
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
  if (usable.some((t) => !t.enabled)) return { status: "partial", reason: tr("capabilities.alcuni_tool_sono_disattivati") };
  return { status: "ready" };
}
