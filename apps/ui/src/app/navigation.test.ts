import { describe, expect, it } from "vitest";
import { groupLabel, hasNavigation, navGroups, routeLabel, visibleRoutes, visibleTabRoutes } from "./navigation";
import { GROUPS, ROUTES, routeById } from "./routes";
import { resolveRoute } from "./router";

const ids = (rs: { id: string }[]) => rs.map((r) => r.id);

describe("visibleRoutes", () => {
  it("shows only the chat in simple mode", () => {
    expect(ids(visibleRoutes(ROUTES, "simple"))).toEqual(["chat"]);
  });
  it("shows the whole table in developer mode", () => {
    expect(visibleRoutes(ROUTES, "developer")).toEqual(ROUTES);
    expect(ROUTES).toHaveLength(19);
  });
  it("keeps every route in the table and resolvable by URL (the shell, not the table, redirects in simple mode)", () => {
    expect(ids(visibleRoutes(ROUTES, "simple"))).not.toContain("runtimes");
    expect(resolveRoute(ROUTES, "#/runtimes")?.route.id).toBe("runtimes");
    expect(resolveRoute(ROUTES, "#/security/policy")?.route.id).toBe("security");
    expect(resolveRoute(ROUTES, "#/settings")?.route.id).toBe("settings");
  });
  it("marks nothing but the chat as simple", () => {
    expect(ROUTES.filter((r) => r.simple).map((r) => r.id)).toEqual(["chat"]);
  });
});

describe("labels", () => {
  it("uses the plainer names in simple mode only", () => {
    expect(routeLabel(routeById("chat"), "simple")).toBe("Chat");
    expect(routeLabel(routeById("chat"), "developer")).toBe("Workspace");
    expect(routeLabel(routeById("agents"), "developer")).toBe("Agenti");
    expect(routeLabel(routeById("models"), "developer")).toBe("Modelli");
  });
  it("renames the inference group in simple mode", () => {
    const g = GROUPS.find((x) => x.id === "inference")!;
    expect(groupLabel(g, "simple")).not.toMatch(/inferenza/i);
    expect(groupLabel(g, "developer")).toBe("Inferenza");
  });
});

describe("hasNavigation", () => {
  it("is false in simple mode (one destination) and true in developer mode", () => {
    expect(hasNavigation(ROUTES, "simple")).toBe(false);
    expect(hasNavigation(ROUTES, "developer")).toBe(true);
  });
});

describe("visibleTabRoutes", () => {
  it("has no tab bar entries in simple mode: there is a single destination", () => {
    expect(visibleTabRoutes(ROUTES, "simple")).toEqual([]);
  });
  it("keeps the four tabs in developer mode, without Insegna (it is in Altro)", () => {
    expect(ids(visibleTabRoutes(ROUTES, "developer"))).toEqual(["home", "chat", "models", "runtimes"]);
  });
});

describe("navGroups", () => {
  it("has only the group of the chat in simple mode, and the shell draws none of them (hasNavigation)", () => {
    const simple = navGroups(GROUPS, ROUTES, "simple");
    expect(simple.map((g) => g.group.id)).toEqual(["work"]);
    expect(simple[0].routes.map((r) => r.id)).toEqual(["chat"]);
  });
  it("keeps every group in developer mode, with every route", () => {
    const all = navGroups(GROUPS, ROUTES, "developer");
    expect(all).toHaveLength(GROUPS.length);
    expect(all.flatMap((g) => g.routes)).toHaveLength(ROUTES.length);
  });
  it("gives the first group no label", () => {
    expect(navGroups(GROUPS, ROUTES, "developer")[0].label).toBeUndefined();
  });
});

describe("Insegna and Addestramento", () => {
  it("are developer sections: not in the simple interface", () => {
    const simple = ids(visibleRoutes(ROUTES, "simple"));
    expect(simple).not.toContain("teach");
    expect(simple).not.toContain("training");
  });
  it("show up in developer mode, Insegna right after the agents", () => {
    const all = ids(visibleRoutes(ROUTES, "developer"));
    expect(all).toContain("training");
    expect(all.indexOf("teach")).toBe(all.indexOf("agents") + 1);
    expect(routeLabel(routeById("teach"), "developer")).toBe("Insegna");
    expect(routeLabel(routeById("training"), "developer")).toBe("Addestramento");
  });
  it("open by URL in either mode", () => {
    expect(resolveRoute(ROUTES, "#/teach")?.route.id).toBe("teach");
    expect(resolveRoute(ROUTES, "#/training/jobs")?.route.id).toBe("training");
  });
  it("never has more than four tabs, so that Altro always fits", () => {
    for (const mode of ["simple", "developer"] as const) {
      const tabs = visibleTabRoutes(ROUTES, mode);
      expect(tabs.length).toBeLessThanOrEqual(4);
      expect(new Set(tabs.map((r) => r.id)).size).toBe(tabs.length);
    }
  });
});
