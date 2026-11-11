import { describe, expect, it } from "vitest";
import { groupLabel, navGroups, routeLabel, visibleRoutes, visibleTabRoutes } from "./navigation";
import { GROUPS, ROUTES, routeById } from "./routes";
import { resolveRoute } from "./router";

const ids = (rs: { id: string }[]) => rs.map((r) => r.id);

describe("visibleRoutes", () => {
  it("shows the six everyday sections in simple mode", () => {
    expect(ids(visibleRoutes(ROUTES, "simple"))).toEqual(["home", "chat", "agents", "teach", "models", "settings"]);
  });
  it("shows the whole table in developer mode", () => {
    expect(visibleRoutes(ROUTES, "developer")).toEqual(ROUTES);
    expect(ROUTES).toHaveLength(16);
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
  it("has no Runtime tab in simple mode, and Insegna fits (four tabs plus Altro)", () => {
    expect(ids(visibleTabRoutes(ROUTES, "simple"))).toEqual(["home", "chat", "teach", "models"]);
  });
  it("keeps the four tabs in developer mode, without Insegna (it is in Altro)", () => {
    expect(ids(visibleTabRoutes(ROUTES, "developer"))).toEqual(["home", "chat", "models", "runtimes"]);
  });
});

describe("navGroups", () => {
  it("drops the groups with no visible route", () => {
    const simple = navGroups(GROUPS, ROUTES, "simple");
    expect(simple.map((g) => g.group.id)).toEqual(["overview", "work", "inference", "control"]);
    expect(simple.find((g) => g.group.id === "work")?.routes.map((r) => r.id)).toEqual(["chat", "agents", "teach"]);
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

describe("Insegna and Addestramento", () => {
  it("shows Insegna right after Assistenti in simple mode, and not the fine-tuning section", () => {
    const simple = ids(visibleRoutes(ROUTES, "simple"));
    expect(simple.indexOf("teach")).toBe(simple.indexOf("agents") + 1);
    expect(simple).not.toContain("training");
    expect(routeLabel(routeById("teach"), "simple")).toBe("Insegna");
  });
  it("shows both in developer mode, Insegna still after the agents", () => {
    const all = ids(visibleRoutes(ROUTES, "developer"));
    expect(all).toContain("training");
    expect(all.indexOf("teach")).toBe(all.indexOf("agents") + 1);
    expect(routeLabel(routeById("training"), "developer")).toBe("Addestramento");
  });
  it("opens by URL in either mode", () => {
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
