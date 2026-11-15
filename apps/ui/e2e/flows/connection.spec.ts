import { expect, openApp, test } from "../support/app";

// The two states of the connection that the demo has no data for (scenarios `offline` and `unpaired`).
test.describe("connection", () => {
  test("unpaired: the pairing screen keeps the app out until the right code is given", async ({ page }) => {
    await openApp(page, "/chat", { scenario: "unpaired" });
    await expect(page.getByRole("heading", { name: "Accoppia questo dispositivo" })).toBeVisible();
    // Nothing of the app is behind it.
    await expect(page.getByRole("textbox", { name: "Messaggio" })).toHaveCount(0);
    const send = page.getByRole("button", { name: "Accoppia" });
    await expect(send).toBeDisabled();
    const code = page.getByRole("textbox", { name: "Codice di pairing" });
    await code.fill("000000");
    await send.click();
    await expect(page.getByText(/codice non valido o scaduto/)).toBeVisible();
    await expect(page.getByRole("heading", { name: "Accoppia questo dispositivo" })).toBeVisible();
    // The code may be typed as it is shown, with a space in the middle; Enter sends it.
    await code.fill("123 456");
    await code.press("Enter");
    await expect(page.getByRole("heading", { name: "Accoppia questo dispositivo" })).toHaveCount(0);
    await expect(page.getByRole("textbox", { name: "Messaggio" })).toBeVisible();
  });

  test("offline: a banner says so, the page around stays up and the retry button is there again after a failed try", async ({ page }) => {
    await openApp(page, "/chat", { scenario: "offline" });
    const banner = page.getByRole("alert").filter({ hasText: "Connessione al workspace persa" });
    await expect(banner).toBeVisible();
    // The chat could not load its list: it says so, with its own button, and does not crash.
    await expect(page.getByText("Questa pagina non si è caricata")).toHaveCount(0);
    await banner.getByRole("button", { name: "Riprova" }).click();
    await expect(banner).toBeVisible();
    await expect(banner.getByRole("button", { name: "Riprova" })).toBeEnabled();
  });
});
