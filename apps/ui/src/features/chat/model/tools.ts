import type { PillTone } from "../../../shared/ui";
import type { ToolStatus } from "./items";

export type ToolCategoryId = "read" | "edit" | "search" | "shell" | "web" | "browser" | "computer" | "mcp" | "agent" | "plan" | "ui" | "other";

export interface ToolCategory {
  id: ToolCategoryId;
  /** Shown as the tooltip and read by screen readers. */
  label: string;
  icon: string;
}

const CATEGORIES: Record<ToolCategoryId, Omit<ToolCategory, "id">> = {
  read: { label: "File", icon: "file" },
  edit: { label: "Modifica file", icon: "edit" },
  search: { label: "Ricerca", icon: "search" },
  shell: { label: "Comandi", icon: "terminal" },
  web: { label: "Web", icon: "cloud" },
  browser: { label: "Browser", icon: "globe" },
  computer: { label: "Desktop", icon: "mouse" },
  mcp: { label: "Server MCP", icon: "link" },
  agent: { label: "Agenti e competenze", icon: "agent" },
  plan: { label: "Piano", icon: "list" },
  ui: { label: "Interfaccia", icon: "panel" },
  other: { label: "Strumento", icon: "tool" },
};

const BY_NAME: Record<string, ToolCategoryId> = {
  read_file: "read", list_dir: "read",
  write_file: "edit", edit_file: "edit", move_file: "edit",
  glob: "search", grep: "search",
  shell: "shell", code: "shell",
  web_fetch: "web", web_search: "web",
  agent: "agent", skill: "agent",
  todo: "plan",
  render_ui: "ui",
};

/** Category of a tool, for the icon and label of its card. */
export function toolCategory(name: string): ToolCategory {
  let id: ToolCategoryId = BY_NAME[name] ?? "other";
  if (id === "other") {
    if (name.startsWith("browser_")) id = "browser";
    else if (name.startsWith("computer_")) id = "computer";
    else if (name.startsWith("mcp__")) id = "mcp";
  }
  return { id, ...CATEGORIES[id] };
}

/** `mcp__github__list_issues` → server and tool; null for any other name. */
export function splitMcpName(name: string): { server: string; tool: string } | null {
  const m = /^mcp__([^_](?:[^_]|_(?!_))*)__(.+)$/.exec(name);
  return m ? { server: m[1], tool: m[2] } : null;
}

/** Name to show on a card: MCP tools read `tool · server`. */
export function toolTitle(name: string): { title: string; sub?: string } {
  const mcp = splitMcpName(name);
  return mcp ? { title: mcp.tool, sub: mcp.server } : { title: name };
}

/** How a tool answer reads: refused, failed, interrupted before running, or fine. */
export function resultStatus(content: string): Exclude<ToolStatus, "running"> {
  if (/^\s*(Negato|Denied)/i.test(content)) return "denied";
  if (/^\s*ERRORE: (strumento non eseguito|Annullato)/.test(content)) return "interrupted";
  if (/^\s*ERRORE/.test(content)) return "failed";
  return "done";
}

export const STATUS_INFO: Record<ToolStatus, { label: string; tone: PillTone }> = {
  running: { label: "In corso", tone: "run" },
  done: { label: "Completato", tone: "ok" },
  failed: { label: "Non riuscito", tone: "crit" },
  denied: { label: "Negato", tone: "crit" },
  interrupted: { label: "Interrotto", tone: "neutral" },
};

/** First non-empty line of a tool result, cut for the card header. */
export function resultPreview(result: string | undefined, max = 140): string {
  const first = (result ?? "").split("\n").find((l) => l.trim() !== "") ?? "";
  const line = first.replace(/^\s*ERRORE:\s*/, "").trim();
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}

/** Elapsed time of a tool call ("<1 s", "2,4 s", "1 min 5 s"); null without both timestamps. */
export function toolDuration(startedAt?: number, endedAt?: number): string | null {
  if (startedAt === undefined || endedAt === undefined || endedAt < startedAt) return null;
  const ms = endedAt - startedAt;
  if (ms < 1000) return "<1 s";
  const s = ms / 1000;
  if (s < 10) return `${s.toFixed(1).replace(".", ",")} s`;
  if (s < 60) return `${Math.round(s)} s`;
  const m = Math.floor(s / 60);
  const rest = Math.round(s % 60);
  return rest ? `${m} min ${rest} s` : `${m} min`;
}

/** File extension for a downloaded screenshot. */
export function imageExtension(mime: string): string {
  const sub = mime.split("/")[1]?.split("+")[0]?.toLowerCase() ?? "png";
  return sub === "jpeg" ? "jpg" : sub;
}
