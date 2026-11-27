import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ApiError } from "../shared/api/http";
import { setStoredUiMode } from "../shared/lib/uiMode";
import { MockTransport } from "../shared/api/mock";
import { DEMO_PAIRING_CODE, transportForScenario } from "../shared/api/mock/scenario";
import { renderApp } from "../test-utils/render";

// The shell around the views: the route table, the simple interface's single chat, the palette and the states of the connection.
describe("App shell: simple interface", () => {
  it("every address ends up in the chat: the root, a section, an old hash, a page that does not exist", async () => {
    for (const route of ["/", "/settings", "/models/catalog", "tools", "/questa-pagina-non-esiste"]) {
      const { unmount } = await renderApp({ route, mode: "simple" });
      await waitFor(() => expect(location.hash, route).toMatch(/^#\/chat(\/ses_\w+)?$/));
      expect(screen.getByRole("textbox", { name: "Messaggio" }), route).toBeInTheDocument();
      await waitFor(() => expect(screen.queryByText("Pagina non trovata"), route).toBeNull());
      unmount();
    }
  });

  it("has no section links, only the menu, which opens the sheet with the mode choice", async () => {
    const { user } = await renderApp({ route: "/chat", mode: "simple" });
    expect(within(screen.getByRole("navigation", { name: "Sezioni" })).queryAllByRole("link")).toHaveLength(0);
    await user.click(within(screen.getByRole("navigation", { name: "Sezioni" })).getByRole("button", { name: "Altro" }));
    const sheet = await screen.findByRole("dialog", { name: "Altro" });
    const mode = within(sheet).getByRole("group", { name: "Modalità dell'interfaccia" });
    expect(within(mode).getByRole("button", { name: "Semplice" })).toHaveAttribute("aria-pressed", "true");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("the menu switches to Developer: every section appears and the address stays", async () => {
    const { user } = await renderApp({ route: "/chat", mode: "simple" });
    await user.click(within(screen.getByRole("navigation", { name: "Sezioni" })).getByRole("button", { name: "Altro" }));
    const sheet = await screen.findByRole("dialog", { name: "Altro" });
    await user.click(within(within(sheet).getByRole("group", { name: "Modalità dell'interfaccia" })).getByRole("button", { name: "Sviluppatore" }));
    await waitFor(() => expect(within(screen.getByRole("navigation", { name: "Sezioni" })).getAllByRole("link").length).toBeGreaterThan(10));
    expect(location.hash).toMatch(/^#\/chat/);
  });
});

describe("App shell: Developer interface", () => {
  it("opens the route that was asked, with its title in the tab and in the page", async () => {
    await renderApp({ route: "/models/catalog", mode: "developer" });
    expect(location.hash).toBe("#/models/catalog");
    expect(screen.getByRole("heading", { name: "Modelli", level: 1 })).toBeInTheDocument();
    expect(document.title).toBe("Modelli · AI Workspace");
    expect(within(screen.getByRole("navigation", { name: "Sezioni" })).getByRole("link", { name: "Modelli" })).toHaveAttribute("aria-current", "page");
  });

  it("the root is the overview", async () => {
    await renderApp({ route: "/", mode: "developer" });
    expect(location.hash).toBe("#/");
    expect(within(screen.getByRole("navigation", { name: "Sezioni" })).getByRole("link", { name: "Panoramica" })).toHaveAttribute("aria-current", "page");
  });

  it("a page that does not exist is a page, not a crash", async () => {
    await renderApp({ route: "/questa-pagina-non-esiste", mode: "developer" });
    expect(await screen.findByRole("heading", { name: "Pagina non trovata" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Vai alla Panoramica" })).toHaveAttribute("href", "#/");
    expect(document.title).toBe("Pagina non trovata · AI Workspace");
  });

  it("the palette opens with Ctrl+K, finds a section and goes there with Enter, and its way back to the simple interface works", async () => {
    const { user } = await renderApp({ route: "/", mode: "developer" });
    await user.keyboard("{Control>}k{/Control}");
    const palette = await screen.findByRole("dialog", { name: "Cerca e comandi" });
    const box = within(palette).getByRole("combobox", { name: "Cerca una sezione o un comando" });
    await user.type(box, "runtime");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(location.hash).toMatch(/^#\/runtimes/));
    expect(screen.queryByRole("dialog", { name: "Cerca e comandi" })).toBeNull();
    await user.keyboard("{Control>}k{/Control}");
    await user.type(within(await screen.findByRole("dialog", { name: "Cerca e comandi" })).getByRole("combobox"), "semplice");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(location.hash).toMatch(/^#\/chat/));
    await waitFor(() => expect(within(screen.getByRole("navigation", { name: "Sezioni" })).queryAllByRole("link")).toHaveLength(0));
  });

  it("the skip link puts the focus on the main area", async () => {
    const { user } = await renderApp({ route: "/", mode: "developer" });
    await user.click(screen.getByRole("button", { name: "Vai al contenuto" }));
    expect(document.activeElement).toBe(screen.getByRole("main"));
  });
});

describe("App shell: the connection", () => {
  it("scenario offline: a banner says the workspace is lost, «Riprova» asks again and the views say they could not load", async () => {
    const api = transportForScenario(new MockTransport(), "offline");
    // The configuration cannot be read, so the mode kept on the device decides: this device is the Developer's.
    setStoredUiMode("developer");
    const { user } = await renderApp({ route: "/models", mode: "developer", api });
    const banner = (await screen.findByText("Connessione al workspace persa", {}, { timeout: 8000 })).closest('[role="alert"]') as HTMLElement;
    expect(banner).not.toBeNull();
    const retry = within(banner).getByRole("button", { name: "Riprova" });
    await user.click(retry);
    // Still offline: after the new try fails the banner is there again, with the button for the next one.
    await screen.findByText("Connessione al workspace persa", {}, { timeout: 8000 });
    // The banner and the failed view each have their own button.
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Riprova" }).length).toBe(2));
    for (const b of screen.getAllByRole("button", { name: "Riprova" })) expect(b).toBeEnabled();
    expect(screen.getByText("Qualcosa non ha funzionato")).toBeInTheDocument();
    // The view around it is still the Models page (a failing request is not a crash).
    expect(screen.getByRole("heading", { name: "Modelli", level: 1 })).toBeInTheDocument();
    expect(screen.queryByText("Questa pagina non si è caricata")).toBeNull();
  }, 20_000);

  it("when the workspace comes back the banner goes away on «Riprova»", async () => {
    const api = new MockTransport();
    const system = vi.spyOn(api, "system").mockRejectedValue(new ApiError(503, "non risponde"));
    const { user } = await renderApp({ route: "/", mode: "developer", api });
    const banner = (await screen.findByText("Connessione al workspace persa", {}, { timeout: 8000 })).closest('[role="alert"]') as HTMLElement;
    system.mockRestore();
    await user.click(within(banner).getByRole("button", { name: "Riprova" }));
    await waitFor(() => expect(screen.queryByText("Connessione al workspace persa")).toBeNull(), { timeout: 8000 });
  }, 20_000);

  it("scenario unpaired: the pairing screen instead of the view; a wrong code stays on it and says why, the right one opens the app", async () => {
    const api = transportForScenario(new MockTransport(), "unpaired");
    const { user } = await renderApp({ route: "/models", mode: "developer", api });
    expect(await screen.findByRole("heading", { name: "Accoppia questo dispositivo" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Modelli", level: 1 })).toBeNull();
    const code = screen.getByRole("textbox", { name: "Codice di pairing" });
    const send = screen.getByRole("button", { name: "Accoppia" });
    expect(send).toBeDisabled();
    await user.type(code, "000000");
    expect(send).toBeEnabled();
    await user.keyboard("{Enter}");
    expect(await screen.findByText(/codice non valido o scaduto/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Accoppia questo dispositivo" })).toBeInTheDocument();
    await user.clear(code);
    await user.type(code, `${DEMO_PAIRING_CODE.slice(0, 3)} ${DEMO_PAIRING_CODE.slice(3)}`);
    await user.click(screen.getByRole("button", { name: "Accoppia" }));
    expect(await screen.findByRole("heading", { name: "Modelli", level: 1 }, { timeout: 8000 })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Accoppia questo dispositivo" })).toBeNull();
  }, 20_000);
});

describe("App shell: the demo", () => {
  it("says it is a demo, because the transport is one", async () => {
    await renderApp({ route: "/chat", mode: "simple" });
    expect(screen.getByText("Demo interattiva")).toBeInTheDocument();
  });
});
