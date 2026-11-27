import { expect, openApp, test } from "../support/app";

test.describe("chat", () => {
  test("send a message, attach a small file, stop the generation, start a new session", async ({ page }) => {
    await openApp(page, "/chat");
    const box = page.getByRole("textbox", { name: "Messaggio" });
    const log = page.getByRole("log", { name: "Conversazione" });

    // A fresh session, so the log starts empty. The simple interface has nothing to choose: no agent, no worktree; the session is the Assistente's.
    await expect(page.getByLabel("Agente della nuova sessione")).toHaveCount(0);
    await expect(page.getByText("Isola in un worktree git")).toHaveCount(0);
    await page.getByRole("button", { name: "Nuova", exact: true }).click();
    await expect(page.getByRole("heading", { name: "Cosa vuoi fare?" })).toBeVisible();
    const list = page.getByRole("navigation", { name: "Elenco delle sessioni" });
    await expect(list.getByRole("button", { name: /^Nuova sessione.*Assistente/ })).toBeVisible();
    // The header carries one control, the model (not a link); the approval mode is in its options; no sandbox.
    const meta = page.getByRole("list", { name: "Come lavora questa sessione" });
    await expect(meta.getByRole("link")).toHaveCount(0);
    await expect(meta.getByText("Sandbox")).toHaveCount(0);
    await meta.getByRole("button", { name: /^Opzioni della conversazione/ }).click();
    await expect(page.getByRole("dialog", { name: "Opzioni della conversazione" }).getByText("Approvazione")).toBeVisible();
    await page.keyboard.press("Escape");

    // Attach a small text file with the message.
    const chooser = page.waitForEvent("filechooser");
    await page.getByRole("button", { name: "Allega un file o un'immagine" }).click();
    await (await chooser).setFiles({ name: "appunti.txt", mimeType: "text/plain", buffer: Buffer.from("Comprare il latte\nChiamare il commercialista\n") });
    const chips = page.getByRole("list", { name: "Allegati del messaggio" });
    await expect(chips.getByText("appunti.txt")).toBeVisible();
    await box.fill("Che cosa c'è in questo file?");
    await page.getByRole("button", { name: "Invia" }).click();
    await expect(log.getByText("Ho ricevuto l'allegato")).toBeVisible();
    await expect(log.getByText("appunti.txt").first()).toBeVisible();
    // The run is over: no stop button, the box is ready again.
    await expect(page.getByRole("button", { name: "Ferma" })).toBeHidden();

    // A normal message starts a long run (a tool waits for confirmation): stop it.
    await box.fill("Ordina i PDF in Downloads");
    await page.getByRole("button", { name: "Invia" }).click();
    const stop = page.getByRole("button", { name: "Ferma" });
    await expect(stop).toBeVisible();
    await expect(log.getByText("Inizio guardando cosa c'è nella cartella Downloads.")).toBeVisible();
    await stop.click();
    await expect(stop).toBeHidden();
    await expect(page.getByRole("button", { name: "Invia" })).toBeVisible();

    // The session was named after the first message; a new one starts empty.
    await expect(list.getByRole("button", { name: /^Che cosa c'è in questo file\?/ })).toBeVisible();
    await page.getByRole("button", { name: "Nuova", exact: true }).click();
    await expect(page.getByRole("heading", { name: "Cosa vuoi fare?" })).toBeVisible();
    await expect(log.getByText("Ordina i PDF in Downloads", { exact: true })).toBeHidden();
  });
});
