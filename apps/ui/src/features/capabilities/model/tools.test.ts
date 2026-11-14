import { describe, expect, it } from "vitest";
import type { ToolInfo } from "../../../shared/api/types";
import { capabilityStatus, countEnabled, groupState, groupTools, isInternalTool, matchesTool, serverTools, shortToolName, switchableTools, toolCategory } from "./tools";

const tool = (name: string, over: Partial<ToolInfo> = {}): ToolInfo => ({
  name, description: `descrizione di ${name}`, source: "builtin", risk: "read", enabled: true, available: true, parameters: {}, ...over,
});
const mcp = (server: string, name: string, over: Partial<ToolInfo> = {}) => tool(`mcp__${server}__${name}`, { source: "mcp", server, risk: "network", ...over });

const ALL: ToolInfo[] = [
  tool("read_file"), tool("write_file", { risk: "write" }), tool("shell", { risk: "exec" }), tool("web_fetch", { risk: "network" }),
  tool("browser_navigate", { risk: "network" }), tool("browser_click", { risk: "computer" }), tool("computer_click", { risk: "computer" }),
  tool("todo", { source: "internal" }), tool("code", { source: "internal" }), tool("mystery"), mcp("github", "list_issues"), mcp("filesystem", "search_files", { enabled: false }),
];

describe("toolCategory", () => {
  it("sorts built-in tools by what they do", () => {
    expect(toolCategory(tool("read_file"))).toBe("files");
    expect(toolCategory(tool("move_file"))).toBe("files");
    expect(toolCategory(tool("copy_file", { risk: "write" }))).toBe("files");
    expect(toolCategory(tool("delete_file", { risk: "write" }))).toBe("files");
    expect(toolCategory(tool("shell"))).toBe("shell");
    expect(toolCategory(tool("web_fetch"))).toBe("web");
    expect(toolCategory(tool("web_search"))).toBe("web");
    expect(toolCategory(tool("browser_snapshot"))).toBe("browser");
    expect(toolCategory(tool("computer_key"))).toBe("computer");
    expect(toolCategory(tool("mystery"))).toBe("other");
  });
  it("puts the tools of the agent loop (source internal) in their own group, whatever their name", () => {
    expect(toolCategory(tool("skill", { source: "internal" }))).toBe("internal");
    expect(toolCategory(tool("render_ui", { source: "internal" }))).toBe("internal");
    expect(isInternalTool(tool("todo", { source: "internal" }))).toBe(true);
    expect(isInternalTool(tool("todo"))).toBe(false);
    expect(switchableTools(ALL).map((t) => t.name)).not.toContain("todo");
    expect(switchableTools(ALL)).toHaveLength(ALL.length - 2);
  });
  it("gives every MCP server its own group", () => {
    expect(toolCategory(mcp("github", "x"))).toBe("mcp:github");
  });
});

describe("shortToolName", () => {
  it("strips the MCP prefix only", () => {
    expect(shortToolName(mcp("github", "list_issues"))).toBe("list_issues");
    expect(shortToolName(tool("read_file"))).toBe("read_file");
    expect(shortToolName({ name: "other", source: "mcp", server: "github" })).toBe("other");
  });
});

describe("groupTools", () => {
  it("orders built-in groups first and MCP servers by name", () => {
    expect(groupTools(ALL).map((g) => g.id)).toEqual(["files", "shell", "web", "browser", "computer", "internal", "other", "mcp:filesystem", "mcp:github"]);
    expect(groupTools(ALL).find((g) => g.id === "internal")?.title).toBe("Interni");
    expect(groupTools(ALL).find((g) => g.id === "mcp:github")?.title).toBe("MCP · github");
  });
  it("filters by text and by risk, dropping empty groups", () => {
    expect(groupTools(ALL, { query: "browser" }).map((g) => g.id)).toEqual(["browser"]);
    expect(groupTools(ALL, { risk: "computer" }).map((g) => g.id)).toEqual(["browser", "computer"]);
    expect(groupTools(ALL, { query: "github", risk: "read" })).toEqual([]);
    expect(groupTools([], {})).toEqual([]);
  });
  it("matches on server and description, ignoring case", () => {
    expect(matchesTool(mcp("GitHub", "x"), "github")).toBe(true);
    expect(matchesTool(tool("a", { description: "Cerca TESTO" }), "testo")).toBe(true);
    expect(matchesTool(tool("a"), "  ")).toBe(true);
    expect(matchesTool(tool("a"), "zzz")).toBe(false);
  });
});

describe("groupState", () => {
  it("tells all, none and some apart", () => {
    expect(groupState([tool("a"), tool("b")])).toBe("all");
    expect(groupState([tool("a", { enabled: false })])).toBe("none");
    expect(groupState([tool("a"), tool("b", { enabled: false })])).toBe("some");
    expect(countEnabled(ALL)).toBe(ALL.length - 1);
  });
});

describe("serverTools", () => {
  it("joins the names the server reports with the registry entries", () => {
    const srv = { name: "github", tools: ["list_issues", "create_issue"] };
    const list = serverTools(srv, ALL);
    expect(list.map((t) => t.name)).toEqual(["list_issues", "create_issue"]);
    expect(list[0].info?.name).toBe("mcp__github__list_issues");
    expect(list[1].info).toBeUndefined();
  });
  it("accepts names that are already prefixed and adds registry tools the server did not list", () => {
    const srv = { name: "github", tools: ["mcp__github__list_issues"] };
    expect(serverTools(srv, [...ALL, mcp("github", "extra")]).map((t) => t.name)).toEqual(["list_issues", "extra"]);
    expect(serverTools({ name: "none", tools: [] }, ALL)).toEqual([]);
  });
});

describe("capabilityStatus", () => {
  it("is absent without tools", () => {
    expect(capabilityStatus([])).toEqual({ status: "absent" });
  });
  it("is unavailable with the reason when no tool can run", () => {
    const off = [tool("computer_click", { available: false, unavailable_reason: "build senza computer-use" }), tool("computer_key", { available: false })];
    expect(capabilityStatus(off)).toEqual({ status: "unavailable", reason: "build senza computer-use" });
  });
  it("is partial when some are unavailable or switched off, off when all are", () => {
    expect(capabilityStatus([tool("a"), tool("b", { available: false, unavailable_reason: "x" })])).toEqual({ status: "partial", reason: "x" });
    expect(capabilityStatus([tool("a"), tool("b", { enabled: false })]).status).toBe("partial");
    expect(capabilityStatus([tool("a", { enabled: false }), tool("b", { enabled: false })]).status).toBe("off");
  });
  it("is ready when everything works", () => {
    expect(capabilityStatus([tool("a"), tool("b")]).status).toBe("ready");
  });
});
