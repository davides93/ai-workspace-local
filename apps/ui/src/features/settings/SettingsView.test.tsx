import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MockTransport } from "../../shared/api/mock";
import { renderApp } from "../../test-utils/render";

// The Settings page (Developer mode: the simple interface has the chat only). Its end-to-end flows are in e2e/flows/settings.spec.ts;
// here the controls one by one, against the demo configuration.
const group = (name: string) => screen.getByRole("group", { name });

describe("Settings view", () => {
  it("is the page of the configuration, in sections, with the layer to write in", async () => {
    await renderApp({ route: "/settings", mode: "developer" });
    expect(screen.getByRole("heading", { name: "Impostazioni", level: 1 })).toBeInTheDocument();
    for (const name of ["Interfaccia", "Agenti", "Sicurezza e rete", "Voce", "Backup e diagnostica"]) expect(screen.getByRole("heading", { name, level: 2 })).toBeInTheDocument();
    const layers = group("Strato in cui scrivere");
    expect(within(layers).getAllByRole("button").map((b) => b.textContent)).toEqual(["globale", "device", "workspace"]);
  });

  it("the interface mode is a choice that saves at once; «Semplice» leaves the page for the chat", async () => {
    const { user, api } = await renderApp({ route: "/settings", mode: "developer" });
    const save = vi.spyOn(api, "setConfig");
    const mode = group("Modalità dell'interfaccia");
    expect(within(mode).getByRole("button", { name: "Sviluppatore" })).toHaveAttribute("aria-pressed", "true");
    await user.click(within(mode).getByRole("button", { name: "Semplice" }));
    await waitFor(() => expect(location.hash).toMatch(/^#\/chat/));
    expect(save).toHaveBeenCalledWith("global", "ui.mode", "simple");
    // The sections go away with the mode.
    expect(within(screen.getByRole("navigation", { name: "Sezioni" })).queryAllByRole("link")).toHaveLength(0);
    expect(screen.queryByRole("heading", { name: "Impostazioni", level: 1 })).toBeNull();
  });

  it("the theme is a choice that applies at once and is kept on the device", async () => {
    const { user } = await renderApp({ route: "/settings", mode: "developer" });
    const theme = group("Tema");
    await user.click(within(theme).getByRole("button", { name: "Scuro" }));
    await waitFor(() => expect(document.documentElement.getAttribute("data-theme")).toBe("dark"));
    expect(within(theme).getByRole("button", { name: "Scuro" })).toHaveAttribute("aria-pressed", "true");
    expect(within(theme).getByRole("button", { name: "Chiaro" })).toHaveAttribute("aria-pressed", "false");
    await user.click(within(theme).getByRole("button", { name: "Chiaro" }));
    await waitFor(() => expect(document.documentElement.getAttribute("data-theme")).toBe("light"));
  });

  it("the language changes the words and <html lang> of the page, nothing else", async () => {
    const { user } = await renderApp({ route: "/settings", mode: "developer" });
    expect(document.documentElement.lang).toBe("it");
    await user.click(within(group("Lingua")).getByRole("button", { name: "English" }));
    expect(await screen.findByRole("heading", { name: "Settings", level: 1 })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("en");
    expect(within(group("Language")).getByRole("button", { name: "English" })).toHaveAttribute("aria-pressed", "true");
    expect(within(screen.getByRole("navigation", { name: "Sections" })).getByRole("link", { name: "Home" })).toBeInTheDocument();
    // And back.
    await user.click(within(group("Language")).getByRole("button", { name: "Italiano" }));
    expect(await screen.findByRole("heading", { name: "Impostazioni", level: 1 })).toBeInTheDocument();
  });

  it("a number is saved with Enter, in the layer that is selected, and can be put back to the default", async () => {
    const { user, api } = await renderApp({ route: "/settings", mode: "developer" });
    const save = vi.spyOn(api, "setConfig");
    const field = screen.getByLabelText(/^Budget di turni per run/);
    await user.clear(field);
    await user.type(field, "12{Enter}");
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save).toHaveBeenCalledWith("global", "agents.budget.max_turns", 12);
    const reset = await screen.findByRole("button", { name: /^Ripristina Budget di turni per run.* nello strato globale$/ });
    expect(field).toHaveValue("12");
    await user.click(reset);
    // The button stays, off, when the layer no longer has its own value.
    await waitFor(() => expect(screen.getByRole("button", { name: /^Ripristina Budget di turni per run/ })).toBeDisabled());
    expect(save).toHaveBeenLastCalledWith("global", "agents.budget.max_turns", null);
  });

  it("a value that is not a number is not sent", async () => {
    const { user, api } = await renderApp({ route: "/settings", mode: "developer" });
    const save = vi.spyOn(api, "setConfig");
    const field = screen.getByLabelText(/^Budget di turni per run/);
    await user.clear(field);
    await user.type(field, "molti{Enter}");
    await new Promise((r) => setTimeout(r, 50));
    expect(save).not.toHaveBeenCalled();
  });

  it("a refused save says why, stores nothing and keeps what was typed so it can be corrected", async () => {
    const api = new MockTransport();
    const { user } = await renderApp({ route: "/settings", mode: "developer", api });
    vi.spyOn(api, "setConfig").mockRejectedValue(new Error("configurazione bloccata"));
    const field = screen.getByLabelText(/^Tempo massimo per run/);
    const stored = (await api.config()).effective["agents.budget.max_seconds"];
    await user.clear(field);
    await user.type(field, "99{Enter}");
    expect(await screen.findByText(/configurazione bloccata/)).toBeInTheDocument();
    expect((await api.config()).effective["agents.budget.max_seconds"]).toBe(stored);
    expect(screen.getByLabelText(/^Tempo massimo per run/)).toHaveValue("99");
  });

  it("the backup is made on request and the page says so", async () => {
    const { user } = await renderApp({ route: "/settings", mode: "developer" });
    // The download itself is the browser's: an anchor with a `download` name is clicked.
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    await user.click(screen.getByRole("button", { name: "Crea e scarica il backup" }));
    expect(await screen.findByText(/Backup creato/)).toBeInTheDocument();
    expect(click).toHaveBeenCalled();
  });
});
