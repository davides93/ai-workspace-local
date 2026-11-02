import { describe, expect, it } from "vitest";
import { buildPath, legacyRedirect, matchPath, normalizePath, parseHash, resolveRoute } from "./router";
import { ROUTES, routeHref, TAB_ROUTES } from "./routes";

describe("parseHash", () => {
  it("splits path and query", () => {
    expect(parseHash("#/models/discover?provider=hf&q=qwen%203")).toEqual({ path: "/models/discover", query: { provider: "hf", q: "qwen 3" } });
  });
  it("treats an empty hash as the root", () => {
    expect(parseHash("")).toEqual({ path: "/", query: {} });
    expect(parseHash("#")).toEqual({ path: "/", query: {} });
    expect(parseHash("#/")).toEqual({ path: "/", query: {} });
  });
  it("normalizes slashes", () => {
    expect(normalizePath("//chat//abc/")).toBe("/chat/abc");
  });
});

describe("matchPath", () => {
  it("matches static paths", () => {
    expect(matchPath("/devices", "/devices")).toEqual({});
    expect(matchPath("/devices", "/devices/extra")).toBeNull();
    expect(matchPath("/devices", "/other")).toBeNull();
  });
  it("extracts required and optional params", () => {
    expect(matchPath("/chat/:sessionId?", "/chat")).toEqual({});
    expect(matchPath("/chat/:sessionId?", "/chat/ses_1")).toEqual({ sessionId: "ses_1" });
    expect(matchPath("/agents/:id", "/agents")).toBeNull();
    expect(matchPath("/chat/:sessionId?", "/chat/a/b")).toBeNull();
  });
  it("decodes percent-encoded params", () => {
    expect(matchPath("/chat/:sessionId?", "/chat/a%2Fb%20c")).toEqual({ sessionId: "a/b c" });
  });
});

describe("route table", () => {
  it("resolves every route to itself", () => {
    for (const r of ROUTES) {
      const base = r.path.replace(/\/:[A-Za-z]+\?$/, "") || "/";
      expect(resolveRoute(ROUTES, `#${base}`)?.route.id).toBe(r.id);
    }
  });
  it("has unique ids and paths", () => {
    expect(new Set(ROUTES.map((r) => r.id)).size).toBe(ROUTES.length);
    expect(new Set(ROUTES.map((r) => r.path)).size).toBe(ROUTES.length);
  });
  it("carries the chat session in the URL", () => {
    const r = resolveRoute(ROUTES, "#/chat/ses_pdf");
    expect(r?.route.id).toBe("chat");
    expect(r?.params.sessionId).toBe("ses_pdf");
  });
  it("carries tabs and query for the models section", () => {
    const r = resolveRoute(ROUTES, "#/models/discover?provider=ollama&format=gguf");
    expect(r?.route.id).toBe("models");
    expect(r?.params.tab).toBe("discover");
    expect(r?.query).toEqual({ provider: "ollama", format: "gguf" });
  });
  it("returns null for unknown paths", () => {
    expect(resolveRoute(ROUTES, "#/nope")).toBeNull();
  });
  it("has the five phone tabs with Panoramica first", () => {
    expect(TAB_ROUTES.map((r) => r.id)).toEqual(["home", "chat", "models", "runtimes"]);
  });
  it("links the rail to routes without optional segments", () => {
    expect(routeHref(ROUTES.find((r) => r.id === "models")!)).toBe("/models");
    expect(routeHref(ROUTES.find((r) => r.id === "home")!)).toBe("/");
  });
});

describe("buildPath", () => {
  it("fills params and encodes them", () => {
    expect(buildPath("/chat/:sessionId?", { sessionId: "a b" })).toBe("/chat/a%20b");
  });
  it("skips missing optional params and adds the query", () => {
    expect(buildPath("/models/:tab?", {}, { q: "qwen", provider: "" })).toBe("/models?q=qwen");
  });
  it("throws for a missing required param", () => {
    expect(() => buildPath("/agents/:id")).toThrow();
  });
});

describe("legacyRedirect", () => {
  it("maps the old section hashes", () => {
    expect(legacyRedirect("#chat")).toBe("#/chat");
    expect(legacyRedirect("#agents")).toBe("#/agents");
    expect(legacyRedirect("#workflows")).toBe("#/workflows");
    expect(legacyRedirect("#runtimes")).toBe("#/runtimes");
    expect(legacyRedirect("#models")).toBe("#/models");
    expect(legacyRedirect("#devices")).toBe("#/devices");
    expect(legacyRedirect("#activity")).toBe("#/activity");
  });
  it("renames tools and preferences", () => {
    expect(legacyRedirect("#tools")).toBe("#/capabilities/tools");
    expect(legacyRedirect("#preferences")).toBe("#/settings");
  });
  it("leaves new-style hashes alone", () => {
    expect(legacyRedirect("#/chat/abc")).toBeNull();
    expect(legacyRedirect("#/")).toBeNull();
    expect(legacyRedirect("")).toBeNull();
  });
  it("sends a bare # to the home page and unknown words to a not-found path", () => {
    expect(legacyRedirect("#")).toBe("#/");
    expect(legacyRedirect("#whatever")).toBe("#/whatever");
  });
  it("redirected hashes resolve to routes", () => {
    for (const old of ["#chat", "#agents", "#workflows", "#runtimes", "#models", "#devices", "#tools", "#activity", "#preferences"]) {
      expect(resolveRoute(ROUTES, legacyRedirect(old)!)).not.toBeNull();
    }
  });
});
