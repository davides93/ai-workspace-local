import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderApp } from "../../test-utils/render";

// A new installation: a runtime that is up and no model on disk (scenario `first-run`).
describe("Models view on a new installation", () => {
  it("says there is no model, and the button leads to the catalog", async () => {
    const { user } = await renderApp({ route: "/models", mode: "developer", scenario: "first-run" });
    expect(await screen.findByRole("heading", { name: "Nessun modello installato", level: 3 })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Apri il catalogo" }));
    await waitFor(() => expect(location.hash).toBe("#/models/catalog"));
    expect(await screen.findByRole("heading", { name: "Catalogo consigliato" })).toBeInTheDocument();
    // Nothing is installed: every card offers the download.
    expect(screen.getAllByRole("button", { name: /^Scarica / }).length).toBeGreaterThan(3);
  });
});
