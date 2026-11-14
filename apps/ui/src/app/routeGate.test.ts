import { describe, expect, it } from "vitest";
import { legacyRedirect, resolveRoute } from "./router";
import { ROUTES } from "./routes";
import { ApiError } from "../shared/api/http";
import { confirmedUiMode, routeGate, SIMPLE_HOME } from "./routeGate";

const gate = (hash: string, mode: "simple" | "developer", configMode: "simple" | "developer" | undefined) =>
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

describe("confirmedUiMode", () => {
  it("is what the configuration says, whatever the device keeps", () => {
    expect(confirmedUiMode({ fromConfig: "developer", status: "success", stored: "simple" })).toBe("developer");
    expect(confirmedUiMode({ fromConfig: "simple", status: "success", stored: "developer" })).toBe("simple");
    // A refresh that fails after an answer keeps the answer.
    expect(confirmedUiMode({ fromConfig: "developer", status: "error", error: new Error("rete"), stored: "simple" })).toBe("developer");
  });

  it("is nothing while the configuration has not answered: the gate waits", () => {
    expect(confirmedUiMode({ fromConfig: undefined, status: "pending", stored: "simple" })).toBeUndefined();
  });

  it("falls back to the mode kept on the device when the configuration cannot be read, so that the gate settles", () => {
    // The request failed for good (network error, 500) or the answer has no valid `ui.mode`.
    const failed = new Error("Failed to fetch");
    expect(confirmedUiMode({ fromConfig: undefined, status: "error", error: failed, stored: "simple" })).toBe("simple");
    expect(confirmedUiMode({ fromConfig: undefined, status: "error", error: failed, stored: "developer" })).toBe("developer");
    expect(confirmedUiMode({ fromConfig: undefined, status: "success", stored: "simple" })).toBe("simple");
    expect(confirmedUiMode({ fromConfig: undefined, status: "success", stored: "developer" })).toBe("developer");
  });

  it("does not take a 401 for an unreadable configuration: the pairing screen is up and the answer will come", () => {
    const unauthorized = new ApiError(401, "non autorizzato");
    expect(confirmedUiMode({ fromConfig: undefined, status: "error", error: unauthorized, stored: "simple" })).toBeUndefined();
  });

  it("never leaves the shell on the skeleton for ever: with an unreadable configuration every address settles", () => {
    const failed = new Error("Failed to fetch");
    for (const stored of ["simple", "developer"] as const) {
      const configMode = confirmedUiMode({ fromConfig: undefined, status: "error", error: failed, stored });
      for (const r of ROUTES) {
        const path = r.path.replace(/\/:[A-Za-z]+\??/g, "") || "/";
        const expected = r.id === "chat" || stored === "developer" ? "show" : "redirect";
        expect(gate(`#${path}`, stored, configMode), `${stored} ${r.id}`).toBe(expected);
      }
      // The initial address (`#/`) and the unknown ones too.
      expect(gate("", stored, configMode)).toBe(stored === "simple" ? "redirect" : "show");
      expect(gate("#/questa-pagina-non-esiste", stored, configMode)).toBe(stored === "simple" ? "redirect" : "show");
    }
  });
});
