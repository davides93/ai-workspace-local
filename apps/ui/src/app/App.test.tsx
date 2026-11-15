import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ApiError } from "../shared/api/http";
import { MockTransport } from "../shared/api/mock";
import { renderApp } from "../test-utils/render";

// The shell around the views: the route table, the simple interface's single chat, the palette and the states of the connection.
describe("App shell: simple interface", () => {
  it("every address ends up in the chat: the root, a section, an old hash, a page that does not exist", async () => {
    for (const route of ["/", "/settings", "/models/catalog", "tools", "/questa-pagina-non-esiste"]) {
      const { unmount } = await renderApp({ route, mode: "simple" });
      await waitFor(() => expect(location.hash, route).toMatch(/^#\/chat(\/ses_\w+)?$/));
      expect(screen.getByRole("textbox", { name: "Messaggio" }), route).toBeInTheDocument();
      expect(screen.queryByText("Pagina non trovata"), route).toBeNull();
      unmount();
    }
  });

  it("has no section links, only the menu, which opens the sheet with the mode choice", async () => {
    const { user } = await renderApp({ route: "/chat", mode: "simple" });
    expect(within(screen.getByRole("navigation", { name: "Sezioni" })).queryAllByRole("link")).toHaveLength(0);
    await user.click(within(screen.getByRole("navigation", { name: "Sezioni" })).getByRole("button", { name: "Menu" }));
    const sheet = await screen.findByRole("dialog", { name: "Menu" });
    const mode = within(sheet).getByRole("group", { name: "Modalità dell'interfaccia" });
    expect(within(mode).getByRole("button", { name: "Semplice" })).toHaveAttribute("aria-pressed", "true");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("the menu switches to Developer: every section appears and the address stays", async () => {
    const { user } = await renderApp({ route: "/chat", mode: "simple" });
    await user.click(within(screen.getByRole("navigation", { name: "Sezioni" })).getByRole("button", { name: "Menu" }));
    const sheet = await screen.findByRole("dialog", { name: "Menu" });
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
  it("when the workspace stops answering a banner says so, and «Riprova» asks again", async () => {
    const api = new MockTransport();
    const system = vi.spyOn(api, "system").mockRejectedValue(new ApiError(503, "non risponde"));
    const { user } = await renderApp({ route: "/", mode: "developer", api });
    const banner = (await screen.findByText("Connessione al workspace persa", {}, { timeout: 8000 })).closest('[role="alert"]') as HTMLElement;
    expect(banner).not.toBeNull();
    system.mockRestore();
    await user.click(within(banner).getByRole("button", { name: "Riprova" }));
    await waitFor(() => expect(screen.queryByText("Connessione al workspace persa")).toBeNull(), { timeout: 8000 });
  }, 20_000);

  it("a browser that is not paired yet gets the pairing screen instead of the view, and a wrong code stays on it", async () => {
    const api = new MockTransport();
    vi.spyOn(api, "system").mockRejectedValue(new ApiError(401, "serve il pairing"));
    const pair = vi.fn().mockRejectedValue(new Error("codice scaduto"));
    (api as unknown as { pair: typeof pair }).pair = pair;
    const { user } = await renderApp({ route: "/", mode: "developer", api });
    expect(await screen.findByRole("heading", { name: "Accoppia questo dispositivo" })).toBeInTheDocument();
    const code = screen.getByRole("textbox", { name: "Codice di pairing" });
    const send = screen.getByRole("button", { name: "Accoppia" });
    expect(send).toBeDisabled();
    await user.type(code, "123456");
    expect(send).toBeEnabled();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(pair).toHaveBeenCalledWith("123456"));
    expect(screen.getByRole("heading", { name: "Accoppia questo dispositivo" })).toBeInTheDocument();
  });
});

describe("App shell: the demo", () => {
  it("says it is a demo, because the transport is one", async () => {
    await renderApp({ route: "/chat", mode: "simple" });
    expect(screen.getByText("Demo interattiva")).toBeInTheDocument();
  });
});
