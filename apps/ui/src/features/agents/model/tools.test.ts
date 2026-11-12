import { describe, expect, it } from "vitest";
import { agents, tools as mockTools } from "../../../shared/api/mock/fixtures";
import type { ToolInfo } from "../../../shared/api/types";
import {
  ALL, INTERNAL_NAMES, INTERNAL_TOOLS, blockedTools, buildCatalog, categoryLabel, categoryOf, expandAll, groupState, groupTools, isAllTools, isSelected,
  normalizeTools, orphanTools, setAllTools, setGroup, summarizeTools, toggleTool,
} from "./tools";

const tool = (name: string, over: Partial<ToolInfo> = {}): ToolInfo => ({
  name, description: `${name} desc`, source: "builtin", risk: "read", enabled: true, available: true, parameters: {}, ...over,
});

/** What an older backend lists: no internal tools (the current one adds them with `source: "internal"`). */
const real: ToolInfo[] = [
  tool("read_file"), tool("write_file", { risk: "write" }), tool("list_dir"), tool("shell", { risk: "exec" }), tool("web_fetch", { risk: "network" }),
  tool("browser_click", { risk: "computer" }), tool("computer_click", { risk: "computer" }),
  tool("mcp__github__list_issues", { source: "mcp", server: "github", risk: "network" }),
  tool("mcp__fs__search_files", { source: "mcp", server: "fs", enabled: false }),
  tool("browser_navigate", { available: false, unavailable_reason: "Chrome non trovato" }),
];

describe("categories", () => {
  it("classifies the tools by family", () => {
    expect(categoryOf(tool("read_file"))).toBe("file");
    expect(categoryOf(tool("grep"))).toBe("file");
    expect(categoryOf(tool("shell"))).toBe("shell");
    expect(categoryOf(tool("web_fetch"))).toBe("web");
    expect(categoryOf(tool("browser_click"))).toBe("browser");
    expect(categoryOf(tool("computer_key"))).toBe("computer");
    expect(categoryOf(tool("todo"))).toBe("internal");
    expect(categoryOf(tool("something_new"))).toBe("other");
  });

  it("names the MCP server from the tool info or from the name", () => {
    expect(categoryOf(tool("x", { source: "mcp", server: "github" }))).toBe("mcp:github");
    expect(categoryOf({ name: "mcp__gitlab__list", source: "builtin" })).toBe("mcp:gitlab");
  });
});

describe("catalogue", () => {
  it("adds the eleven internal tools that the backend does not list", () => {
    const names = buildCatalog(real).map((t) => t.name);
    for (const n of INTERNAL_NAMES) expect(names).toContain(n);
    expect(INTERNAL_TOOLS).toHaveLength(11);
  });

  it("does not duplicate internal tools a catalogue (like the demo) already lists", () => {
    const names = buildCatalog(mockTools).map((t) => t.name);
    expect(names.filter((n) => n === "todo")).toHaveLength(1);
    expect(new Set(names).size).toBe(names.length);
  });

  it("takes the internal tools from the catalogue when it lists them (source internal), with its own text", () => {
    const listed = [...real, tool("todo", { source: "internal", description: "Piano di lavoro (dal backend)" }), tool("code", { source: "internal", risk: "read" })];
    const cat = buildCatalog(listed);
    expect(cat.find((t) => t.name === "todo")).toMatchObject({ category: "internal", description: "Piano di lavoro (dal backend)", disabled: false });
    expect(cat.filter((t) => t.category === "internal").map((t) => t.name).sort()).toEqual([...INTERNAL_NAMES].sort());
    expect(new Set(cat.map((t) => t.name)).size).toBe(cat.length);
  });

  it("names the group Interni and never calls an internal tool disattivato", () => {
    expect(categoryLabel("internal")).toBe("Interni");
    const cat = buildCatalog([tool("skill", { source: "internal", enabled: false })]);
    expect(cat.find((t) => t.name === "skill")?.disabled).toBe(false);
  });

  it("flags disabled and unavailable tools", () => {
    const cat = buildCatalog(real);
    expect(cat.find((t) => t.name === "mcp__fs__search_files")?.disabled).toBe(true);
    expect(cat.find((t) => t.name === "browser_navigate")?.unavailable).toBe("Chrome non trovato");
  });
});

describe("groups", () => {
  const cat = buildCatalog(real);

  it("orders families, MCP servers, internal tools", () => {
    expect(groupTools(cat).map((g) => g.id)).toEqual(["file", "shell", "web", "browser", "computer", "mcp:fs", "mcp:github", "internal"]);
  });

  it("filters by name, description or group label", () => {
    expect(groupTools(cat, "github").flatMap((g) => g.tools.map((t) => t.name))).toEqual(["mcp__github__list_issues"]);
    expect(groupTools(cat, "SHELL").map((g) => g.id)).toEqual(["shell"]);
    expect(groupTools(cat, "interni").map((g) => g.id)).toEqual(["internal"]);
    expect(groupTools(cat, "zzz")).toEqual([]);
  });
});

describe("selection", () => {
  const cat = buildCatalog(real);

  it("treats * as every tool", () => {
    expect(isAllTools(["*"])).toBe(true);
    expect(isSelected(["*"], "anything")).toBe(true);
    expect(isSelected(["read_file"], "write_file")).toBe(false);
    expect(normalizeTools(["*", "read_file"])).toEqual(["*"]);
    expect(normalizeTools(["a", "a", " b ", ""])).toEqual(["a", "b"]);
  });

  it("turning * off keeps every tool, the internal ones included", () => {
    const explicit = setAllTools([ALL], false, cat);
    expect(explicit).toEqual(expandAll(cat));
    for (const n of INTERNAL_NAMES) expect(explicit).toContain(n);
    expect(setAllTools(["a"], false, cat)).toEqual(["a"]);
    expect(setAllTools(["a"], true, cat)).toEqual(["*"]);
  });

  it("toggles one tool and a whole group", () => {
    expect(toggleTool(["a"], "b", true)).toEqual(["a", "b"]);
    expect(toggleTool(["a", "b"], "a", false)).toEqual(["b"]);
    expect(toggleTool(["a"], "a", true)).toEqual(["a"]);
    expect(setGroup(["x", "a"], ["a", "b"], true)).toEqual(["x", "a", "b"]);
    expect(setGroup(["x", "a", "b"], ["a", "b"], false)).toEqual(["x"]);
  });

  it("reports the state of a group", () => {
    expect(groupState(["a"], ["a", "b"])).toBe("some");
    expect(groupState(["a", "b"], ["a", "b"])).toBe("all");
    expect(groupState([], ["a", "b"])).toBe("none");
    expect(groupState(["*"], ["a", "b"])).toBe("all");
    expect(groupState(["a"], [])).toBe("none");
  });

  it("finds names the catalogue no longer knows", () => {
    expect(orphanTools(["read_file", "mcp__gone__x", "todo", "*"], cat)).toEqual(["mcp__gone__x"]);
  });

  it("summarizes the selection by risk", () => {
    const s = summarizeTools(["read_file", "write_file", "shell", "todo"], cat);
    expect(s).toMatchObject({ count: 4, total: cat.length });
    expect(s.byRisk).toEqual({ read: 2, write: 1, exec: 1 });
    expect(summarizeTools(["*"], cat).count).toBe(cat.length);
  });
});

describe("tools an agent can never use", () => {
  const cat = buildCatalog(real);

  it("lists write, exec and computer tools in Piano mode", () => {
    const names = blockedTools(["read_file", "write_file", "shell", "browser_click"], cat, "plan", "workspace-write").map((b) => b.name);
    expect(names).toEqual(["write_file", "shell", "browser_click"]);
  });

  it("lists file writers in a read-only sandbox", () => {
    expect(blockedTools(["read_file", "write_file"], cat, "ask", "read-only").map((b) => b.name)).toEqual(["write_file"]);
  });

  it("is quiet when nothing conflicts", () => {
    expect(blockedTools(["read_file", "write_file"], cat, "ask", "workspace-write")).toEqual([]);
  });
});

describe("seeded agents", () => {
  it("only name tools the picker knows (or *)", () => {
    const cat = buildCatalog(mockTools);
    for (const a of agents) expect(orphanTools(a.tools, cat), a.id).toEqual([]);
  });
});
