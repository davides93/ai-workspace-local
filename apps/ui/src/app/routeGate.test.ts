import { describe, expect, it } from "vitest";
import { legacyRedirect, resolveRoute } from "./router";
import { ROUTES } from "./routes";
import { ApiError } from "../shared/api/http";
import { configModeOf, routeGate, SIMPLE_HOME, type ConfigMode } from "./routeGate";

const gate = (hash: string, mode: "simple" | "developer", configMode: ConfigMode) =>
  routeGate({ routeId: resolveRoute(ROUTES, hash)?.route.id, mode, configMode });

describe("routeGate", () => {
  it("sends every address but the chat to the chat once the configuration confirms simple mode", () => {
    for (const r of ROUTES.filter((x) => x.id !== "chat")) {
      const path = r.path.replace(/\/:[A-Za-z]+\??/g, "") || "/";
      expect(gate(`#${path}`, "simple", "simple"), r.id).toBe("redirect");
    }
    expect(SIMPLE_HOME).toBe("/chat");
  });

  it("keeps the chat, with or without a session, whatever the mode", () => {
    for (const mode of ["simple", "developer"] as const) {
      for (const cfg of ["simple", "developer", undefined] as const) {
        expect(gate("#/chat", mode, cfg)).toBe("show");
        expect(gate("#/chat/ses_pdf", mode, cfg)).toBe("show");
      }
    }
  });

  it("sends the \"not found\" page to the chat too, but only once simple mode is confirmed", () => {
    expect(gate("#/questa-pagina-non-esiste", "simple", "simple")).toBe("redirect");
    expect(gate("#/questa-pagina-non-esiste", "simple", undefined)).toBe("wait");
    expect(gate("#/questa-pagina-non-esiste", "developer", "developer")).toBe("show");
  });

  it("never redirects a Developer person who has no mode stored on this device while the configuration loads", () => {
    // The device shows `simple` (its default) and the configuration has not answered yet: hold back, do not redirect.
    expect(gate("#/runtimes", "simple", undefined)).toBe("wait");
    // The configuration answers `developer`; the shown mode follows one render later: still no redirect.
    expect(gate("#/runtimes", "simple", "developer")).toBe("wait");
    // Caught up: the route opens.
    expect(gate("#/runtimes", "developer", "developer")).toBe("show");
  });

  it("does not redirect on a stale Developer value either: it waits until the shown mode follows the configuration", () => {
    expect(gate("#/runtimes", "developer", undefined)).toBe("show");
    expect(gate("#/runtimes", "developer", "simple")).toBe("wait");
    expect(gate("#/runtimes", "simple", "simple")).toBe("redirect");
  });

  it("opens all 19 routes in Developer mode, with the configuration confirming it", () => {
    expect(ROUTES).toHaveLength(19);
    for (const r of ROUTES) expect(routeGate({ routeId: r.id, mode: "developer", configMode: "developer" }), r.id).toBe("show");
  });

  it("applies to the old hashes after they are translated", () => {
    const cases: [string, string][] = [
      ["#chat", "show"], ["#agents", "redirect"], ["#workflows", "redirect"], ["#runtimes", "redirect"], ["#models", "redirect"],
      ["#devices", "redirect"], ["#tools", "redirect"], ["#activity", "redirect"], ["#preferences", "redirect"], ["#anything-else", "redirect"],
    ];
    for (const [old, expected] of cases) {
      const target = legacyRedirect(old);
      expect(target, old).not.toBeNull();
      expect(gate(target!, "simple", "simple"), `${old} → ${target}`).toBe(expected);
      // Developer mode: the translated address opens its route.
      expect(gate(target!, "developer", "developer"), `${old} (developer)`).toBe("show");
    }
  });

  it("treats an empty hash like the home page: redirected in simple mode", () => {
    expect(gate("", "simple", "simple")).toBe("redirect");
    expect(gate("", "developer", "developer")).toBe("show");
  });
});

describe("configModeOf", () => {
  it("is the mode the configuration reports", () => {
    expect(configModeOf({ fromConfig: "developer", status: "success" })).toBe("developer");
    expect(configModeOf({ fromConfig: "simple", status: "success" })).toBe("simple");
    // A refresh that fails after an answer keeps the answer.
    expect(configModeOf({ fromConfig: "developer", status: "error", error: new Error("rete") })).toBe("developer");
  });

  it("is nothing while the configuration has not answered: the gate waits", () => {
    expect(configModeOf({ fromConfig: undefined, status: "pending" })).toBeUndefined();
  });

  it("is `unreadable` when the request failed for good or the answer has no valid `ui.mode`", () => {
    expect(configModeOf({ fromConfig: undefined, status: "error", error: new TypeError("Failed to fetch") })).toBe("unreadable");
    expect(configModeOf({ fromConfig: undefined, status: "error", error: new ApiError(500, "errore del server") })).toBe("unreadable");
    expect(configModeOf({ fromConfig: undefined, status: "success" })).toBe("unreadable");
  });

  it("does not take a 401 for an unreadable configuration: the pairing screen is up and the answer will come", () => {
    expect(configModeOf({ fromConfig: undefined, status: "error", error: new ApiError(401, "non autorizzato") })).toBeUndefined();
  });
});

describe("routeGate with a configuration that cannot be read", () => {
  it("goes by the mode the device shows: simple sends every address but the chat to the chat", () => {
    for (const r of ROUTES) {
      const path = r.path.replace(/\/:[A-Za-z]+\??/g, "") || "/";
      expect(gate(`#${path}`, "simple", "unreadable"), `simple ${r.id}`).toBe(r.id === "chat" ? "show" : "redirect");
    }
    // The chat itself (with and without a session), so there is no loop.
    expect(gate("#/chat", "simple", "unreadable")).toBe("show");
    expect(gate("#/chat/ses_1", "simple", "unreadable")).toBe("show");
    // The initial address (`#/`), the old hashes and the unknown ones too: neither the Overview nor "not found" shows in the simple shell.
    expect(gate("", "simple", "unreadable")).toBe("redirect");
    expect(gate("#tools", "simple", "unreadable")).toBe("redirect");
    expect(gate("#/questa-pagina-non-esiste", "simple", "unreadable")).toBe("redirect");
    // A device that kept no mode shows the default (`simple`): the first run with a slow or unreachable engine lands in the chat.
    expect(gate("#/runtimes", "simple", "unreadable")).toBe("redirect");
  });

  it("keeps opening the address that was asked for on a device that shows Developer mode: no change for Developer", () => {
    for (const r of ROUTES) {
      const path = r.path.replace(/\/:[A-Za-z]+\??/g, "") || "/";
      expect(gate(`#${path}`, "developer", "unreadable"), `developer ${r.id}`).toBe("show");
    }
    expect(gate("", "developer", "unreadable")).toBe("show");
    expect(gate("#tools", "developer", "unreadable")).toBe("show");
    expect(gate("#/questa-pagina-non-esiste", "developer", "unreadable")).toBe("show");
  });

  it("redirects as usual once a later answer says `simple`, and keeps waiting while the pairing screen is up", () => {
    expect(gate("#/runtimes", "simple", configModeOf({ fromConfig: "simple", status: "success" }))).toBe("redirect");
    expect(gate("#/runtimes", "simple", configModeOf({ fromConfig: undefined, status: "error", error: new ApiError(401, "x") }))).toBe("wait");
    expect(gate("#/runtimes", "simple", configModeOf({ fromConfig: undefined, status: "pending" }))).toBe("wait");
  });
});
