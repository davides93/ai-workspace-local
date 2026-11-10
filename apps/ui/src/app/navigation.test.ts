import { describe, expect, it } from "vitest";
import { groupLabel, navGroups, routeLabel, visibleRoutes, visibleTabRoutes } from "./navigation";
import { GROUPS, ROUTES, routeById } from "./routes";
import { resolveRoute } from "./router";

const ids = (rs: { id: string }[]) => rs.map((r) => r.id);

describe("visibleRoutes", () => {
  it("shows the five everyday sections in simple mode", () => {
    expect(ids(visibleRoutes(ROUTES, "simple"))).toEqual(["home", "chat", "agents", "models", "settings"]);
  });
  it("shows the whole table in developer mode", () => {
    expect(visibleRoutes(ROUTES, "developer")).toEqual(ROUTES);
    expect(ROUTES).toHaveLength(15);
  });
  it("hides routes without the simple marker, but they still resolve by URL", () => {
    expect(ids(visibleRoutes(ROUTES, "simple"))).not.toContain("runtimes");
    expect(resolveRoute(ROUTES, "#/runtimes")?.route.id).toBe("runtimes");
    expect(resolveRoute(ROUTES, "#/security/policy")?.route.id).toBe("security");
  });
});

describe("labels", () => {
  it("uses the plainer names in simple mode only", () => {
    expect(routeLabel(routeById("chat"), "simple")).toBe("Chat");
    expect(routeLabel(routeById("chat"), "developer")).toBe("Workspace");
    expect(routeLabel(routeById("agents"), "simple")).toBe("Assistenti");
    expect(routeLabel(routeById("agents"), "developer")).toBe("Agenti");
    expect(routeLabel(routeById("models"), "simple")).toBe("Modelli");
  });
  it("renames the inference group in simple mode", () => {
    const g = GROUPS.find((x) => x.id === "inference")!;
    expect(groupLabel(g, "simple")).not.toMatch(/inferenza/i);
    expect(groupLabel(g, "developer")).toBe("Inferenza");
  });
});

describe("visibleTabRoutes", () => {
  it("has no Runtime tab in simple mode", () => {
    expect(ids(visibleTabRoutes(ROUTES, "simple"))).toEqual(["home", "chat", "models"]);
  });
  it("keeps the four tabs in developer mode", () => {
    expect(ids(visibleTabRoutes(ROUTES, "developer"))).toEqual(["home", "chat", "models", "runtimes"]);
  });
});

describe("navGroups", () => {
  it("drops the groups with no visible route", () => {
    const simple = navGroups(GROUPS, ROUTES, "simple");
    expect(simple.map((g) => g.group.id)).toEqual(["overview", "work", "inference", "control"]);
    expect(simple.find((g) => g.group.id === "work")?.routes.map((r) => r.id)).toEqual(["chat", "agents"]);
  });
  it("keeps every group in developer mode, with every route", () => {
    const all = navGroups(GROUPS, ROUTES, "developer");
    expect(all).toHaveLength(GROUPS.length);
    expect(all.flatMap((g) => g.routes)).toHaveLength(ROUTES.length);
  });
  it("gives the first group no label", () => {
    expect(navGroups(GROUPS, ROUTES, "simple")[0].label).toBeUndefined();
  });
});
