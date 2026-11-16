import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderApp } from "../../test-utils/render";

// The Models section in Developer mode (the simple interface has no such page: any address leads to the chat).
const tab = (name: RegExp | string) => screen.getByRole("tab", { name });
const search = () => document.getElementById("inst-q") as HTMLInputElement;

describe("Models view", () => {
  it("opens on the models that can run here, with the tabs of the section and one card per model", async () => {
    await renderApp({ route: "/models", mode: "developer" });
    expect(screen.getByRole("heading", { name: "Modelli", level: 1 })).toBeInTheDocument();
    expect(screen.getAllByRole("tab").map((t) => t.textContent?.replace(/\d+$/, ""))).toEqual(["Galleria", "Disponibili qui", "Profili", "Scopri", "Download", "Catalogo"]);
    expect(tab(/^Disponibili qui/)).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("heading", { name: "Modelli installati" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Qwen3.5 35B-A3B Instruct", level: 3 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Elimina Qwen3 0.6B" })).toBeInTheDocument();
  });

  it("the search narrows the cards as the person types, says when nothing matches, and clearing it brings them back", async () => {
    const { user } = await renderApp({ route: "/models", mode: "developer" });
    const before = screen.getAllByRole("heading", { level: 3 }).length;
    expect(before).toBeGreaterThan(3);
    await user.type(search(), "whisper");
    await waitFor(() => expect(screen.getAllByRole("heading", { level: 3 })).toHaveLength(1));
    expect(screen.getByRole("heading", { name: "Whisper large-v3-turbo", level: 3 })).toBeInTheDocument();
    await user.clear(search());
    await user.type(search(), "zzz-nessuno");
    expect(await screen.findByRole("heading", { name: "Nessun modello corrisponde", level: 3 })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Whisper large-v3-turbo" })).toBeNull();
    await user.clear(search());
    await waitFor(() => expect(screen.getAllByRole("heading", { level: 3 })).toHaveLength(before));
  });

  it("a tab changes the address and the content; the tabs move with the arrow keys", async () => {
    const { user } = await renderApp({ route: "/models", mode: "developer" });
    await user.click(tab("Catalogo"));
    await waitFor(() => expect(location.hash).toBe("#/models/catalog"));
    expect(await screen.findByRole("heading", { name: "Catalogo consigliato" })).toBeInTheDocument();
    expect(tab("Catalogo")).toHaveAttribute("aria-selected", "true");
    expect(tab(/^Disponibili qui/)).toHaveAttribute("aria-selected", "false");
    tab("Catalogo").focus();
    await user.keyboard("{ArrowLeft}");
    await waitFor(() => expect(location.hash).toBe("#/models/downloads"));
    expect(tab("Download")).toHaveAttribute("aria-selected", "true");
  });

  it("an address with a tab opens that tab", async () => {
    await renderApp({ route: "/models/catalog", mode: "developer" });
    expect(tab("Catalogo")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("heading", { name: "Catalogo consigliato" })).toBeInTheDocument();
  });

  it("deleting asks first, spells out what goes, can be cancelled, and then takes the model off the list", async () => {
    const { user } = await renderApp({ route: "/models", mode: "developer" });
    await user.click(screen.getByRole("button", { name: "Elimina Qwen3 0.6B" }));
    const dialog = await screen.findByRole("dialog", { name: "Elimina il modello" });
    expect(within(dialog).getByText("Qwen3 0.6B")).toBeInTheDocument();
    expect(within(dialog).getByText("Spazio liberato")).toBeInTheDocument();
    // The focus starts on the safe choice.
    await waitFor(() => expect(document.activeElement).toBe(within(dialog).getByRole("button", { name: "Annulla" })));
    await user.click(within(dialog).getByRole("button", { name: "Annulla" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByRole("heading", { name: "Qwen3 0.6B", level: 3 })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Elimina Qwen3 0.6B" }));
    await user.click(within(await screen.findByRole("dialog", { name: "Elimina il modello" })).getByRole("button", { name: "Elimina dal disco" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Qwen3 0.6B", level: 3 })).toBeNull());
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("a download from the catalog shows up as a meter in the Download tab", async () => {
    const { user } = await renderApp({ route: "/models/catalog", mode: "developer" });
    await user.click(screen.getByRole("button", { name: "Scarica 256 MB" }));
    await user.click(tab(/^Download/));
    const meter = await screen.findByRole("meter", { name: /^Avanzamento di / }, { timeout: 10_000 });
    expect(meter).toHaveAttribute("aria-valuemin", "0");
    expect(meter).toHaveAttribute("aria-valuemax");
  });
});
