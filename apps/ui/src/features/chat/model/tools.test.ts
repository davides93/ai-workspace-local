import { describe, expect, it } from "vitest";
import { imageExtension, resultPreview, resultStatus, splitMcpName, toolCategory, toolDuration, toolTitle } from "./tools";

describe("toolCategory", () => {
  it("groups the built-in tools", () => {
    expect(toolCategory("read_file")).toMatchObject({ id: "read", icon: "file" });
    expect(toolCategory("list_dir").id).toBe("read");
    expect(toolCategory("write_file")).toMatchObject({ id: "edit", icon: "edit" });
    expect(toolCategory("edit_file").id).toBe("edit");
    expect(toolCategory("grep").id).toBe("search");
    expect(toolCategory("shell")).toMatchObject({ id: "shell", icon: "terminal" });
    expect(toolCategory("web_fetch").id).toBe("web");
    expect(toolCategory("todo").id).toBe("plan");
    expect(toolCategory("render_ui").id).toBe("ui");
  });
  it("groups browser, computer-use and MCP tools by prefix", () => {
    expect(toolCategory("browser_navigate")).toMatchObject({ id: "browser", icon: "globe" });
    expect(toolCategory("computer_screenshot")).toMatchObject({ id: "computer", icon: "mouse" });
    expect(toolCategory("mcp__github__list_issues")).toMatchObject({ id: "mcp", icon: "link" });
  });
  it("falls back to a generic tool", () => {
    expect(toolCategory("qualcosa_di_nuovo")).toMatchObject({ id: "other", icon: "tool" });
  });
});

describe("MCP names", () => {
  it("splits server and tool, also with underscores in the server name", () => {
    expect(splitMcpName("mcp__github__list_issues")).toEqual({ server: "github", tool: "list_issues" });
    expect(splitMcpName("mcp__my_server__do__thing")).toEqual({ server: "my_server", tool: "do__thing" });
    expect(splitMcpName("shell")).toBeNull();
    expect(toolTitle("mcp__github__list_issues")).toEqual({ title: "list_issues", sub: "github" });
    expect(toolTitle("shell")).toEqual({ title: "shell" });
  });
});

describe("result reading", () => {
  it("classifies an answer", () => {
    expect(resultStatus("Negato dall'utente.")).toBe("denied");
    expect(resultStatus("negato dalla policy")).toBe("denied");
    expect(resultStatus("ERRORE: file non trovato")).toBe("failed");
    expect(resultStatus("ERRORE: strumento non eseguito, il run è stato interrotto prima.")).toBe("interrupted");
    expect(resultStatus("7 file")).toBe("done");
    expect(resultStatus("")).toBe("done");
  });
  it("previews the first line without the ERRORE prefix", () => {
    expect(resultPreview("\n\nERRORE: boom\naltro")).toBe("boom");
    expect(resultPreview("a".repeat(300), 20)).toHaveLength(20);
    expect(resultPreview(undefined)).toBe("");
  });
  it("formats the duration of a call", () => {
    expect(toolDuration(1000, 1400)).toBe("<1 s");
    expect(toolDuration(0, 2400)).toBe("2,4 s");
    expect(toolDuration(0, 42_000)).toBe("42 s");
    expect(toolDuration(0, 65_000)).toBe("1 min 5 s");
    expect(toolDuration(0, 120_000)).toBe("2 min");
    expect(toolDuration(undefined, 5)).toBeNull();
    expect(toolDuration(10, 5)).toBeNull();
  });
  it("picks a file extension for a screenshot", () => {
    expect(imageExtension("image/png")).toBe("png");
    expect(imageExtension("image/jpeg")).toBe("jpg");
    expect(imageExtension("image/svg+xml")).toBe("svg");
  });
});
