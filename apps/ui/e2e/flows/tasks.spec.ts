import type { Page } from "@playwright/test";
import { expect, openApp, test } from "../support/app";

// Background tasks on the Proposte page (Developer mode). The demo has a task at work, three paused (quiet hours, a spent
// budget, a restart) and two finished; nothing in it moves by itself, so each test drives what it needs.

const panel = (page: Page) => page.getByRole("region", { name: "Lavori in background" });
const card = (page: Page, title: string) => panel(page).getByRole("article", { name: title });
const openInbox = (page: Page, scenario?: "first-run") => openApp(page, "/inbox", { mode: "developer", scenario });

const RUN = "Archiviare le fatture di ottobre";
const QUIET = "Confrontare le offerte dei fornitori";
const BUDGET = "Prenotare la sala riunioni per giovedì";
const RESTART = "Sistemare il test del parser GGUF";

test("a task at work: pause, tell it something, resume, allow one confirmation and deny the next, cancel", async ({ page }) => {
  await openInbox(page);
  const run = card(page, RUN);
  await expect(run.getByText("In corso", { exact: true })).toBeVisible();
  await expect(run.getByText("3 passi · 4 min · ultimo strumento list_dir")).toBeVisible();
  await expect(run.getByText("Ho trovato 4 file che sembrano fatture")).toBeVisible();
  const given = run.getByRole("list", { name: "Istruzioni date" });
  await expect(given.getByRole("listitem").filter({ hasText: "Metti le fatture di Rossi Srl" }).getByText("Letta", { exact: true })).toBeVisible();

  // 1. Pause: it finishes the step it is on, then stops.
  await run.getByRole("button", { name: "Pausa", exact: true }).click();
  await expect(run.getByText("Si sta fermando", { exact: true })).toBeVisible();
  await expect(run.getByText("Pausa chiesta: si ferma quando finisce il passo che sta facendo.")).toBeVisible();
  await expect(run.getByRole("button", { name: "Pausa", exact: true })).toBeHidden();
  await expect(run.getByText("In pausa", { exact: true })).toBeVisible();
  await expect(run.getByText("L'hai messo in pausa tu", { exact: false })).toBeVisible();

  // 2. Something said while it is paused is kept, waiting for the next step.
  const say = run.getByRole("textbox", { name: "Istruzione" });
  await say.fill("Guarda anche la cartella Documenti.");
  await expect(run.getByText(/^35 \/ 4000$/)).toBeVisible();
  await run.getByRole("button", { name: "Invia", exact: true }).click();
  await expect(say).toHaveValue("");
  const mine = given.getByRole("listitem").filter({ hasText: "Guarda anche la cartella Documenti." });
  await expect(mine.getByText("In attesa", { exact: true })).toBeVisible();

  // 3. Resume: it reads the instruction, then asks for a confirmation.
  await run.getByRole("button", { name: "Riprendi", exact: true }).click();
  await expect(mine.getByText("Letta", { exact: true })).toBeVisible();
  const ask = run.getByRole("group", { name: "Conferma in attesa" });
  await expect(run.getByText("Aspetta una conferma", { exact: true })).toBeVisible();
  await expect(ask.getByRole("article", { name: "shell" })).toBeVisible();
  await expect(ask.getByText("Il comando sposta 2 file dalla cartella Scaricati a Documenti/Fatture/Rossi.")).toBeVisible();
  // Nothing here lasts for the whole session: each request is decided on its own.
  await expect(ask.getByRole("button", { name: /per la sessione/ })).toHaveCount(0);
  await ask.getByRole("button", { name: "Consenti", exact: true }).click();

  // 4. The next confirmation is denied: the task goes on without it.
  await expect(ask.getByRole("article", { name: "write_file" })).toBeVisible();
  await ask.getByRole("button", { name: "Nega", exact: true }).click();
  await expect(ask).toBeHidden();
  await expect(run.getByText("In corso", { exact: true })).toBeVisible();
  await expect(run.getByText("Ok, quello non lo faccio: vado avanti con il resto.")).toBeVisible();
  await run.getByText(/^Cronologia \(\d+\)$/).click();
  await expect(run.getByText("Hai negato «write_file»: il lavoro va avanti senza.")).toBeVisible();

  // 5. Cancel: the dialog asks first, and leaving it keeps the task.
  await run.getByRole("button", { name: "Annulla", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Annullare il lavoro?" });
  await expect(dialog.getByText(`«${RUN}» si ferma subito`, { exact: false })).toBeVisible();
  await dialog.getByRole("button", { name: "Lascia stare" }).click();
  await expect(dialog).toBeHidden();
  await expect(run).toBeVisible();
  await run.getByRole("button", { name: "Annulla", exact: true }).click();
  await dialog.getByRole("button", { name: "Annulla il lavoro" }).click();
  await expect(run).toBeHidden();
  const finished = panel(page).getByRole("listitem").filter({ hasText: RUN });
  await expect(finished.getByText("Annullato", { exact: true })).toBeVisible();

  // A finished task takes no more instructions.
  await finished.getByText("Dettagli", { exact: true }).click();
  await expect(finished.getByRole("textbox", { name: "Istruzione" })).toBeDisabled();
  await expect(finished.getByRole("button", { name: "Invia", exact: true })).toBeDisabled();
});

test("quiet hours: a plain resume is refused with the engine's sentence, resuming anyway goes ahead", async ({ page }) => {
  await openInbox(page);
  const quiet = card(page, QUIET);
  await expect(quiet.getByText("È il tuo orario di silenzio (fino alle 07:00): riparte da solo quando finisce.")).toBeVisible();
  await expect(quiet.getByText("In pausa", { exact: true })).toBeVisible();
  await quiet.getByRole("button", { name: "Riprendi", exact: true }).click();
  // The sentence of the 409 is in a toast, and stays in the card where the decision is taken.
  await expect(page.getByRole("alert").filter({ hasText: "Se vuoi che continui adesso usa «Riprendi comunque»" })).toBeVisible();
  await expect(quiet.locator(".tk-refusal")).toContainText("È ancora l'orario di silenzio");
  await expect(quiet.getByText("In pausa", { exact: true })).toBeVisible();
  await quiet.getByRole("button", { name: "Riprendi comunque", exact: true }).click();
  await expect(quiet.getByText("In corso", { exact: true })).toBeVisible();
  await expect(quiet.locator(".tk-refusal")).toHaveCount(0);
  await quiet.getByText(/^Cronologia \(\d+\)$/).click();
  await expect(quiet.getByText("Ripreso comunque, passando sopra la regola.")).toBeVisible();
});

test("after a restart nothing starts by itself; a spent budget needs more time", async ({ page }) => {
  await openInbox(page);
  const restart = card(page, RESTART);
  await expect(restart.getByText("Si è interrotto perché l'app è stata riavviata", { exact: false })).toBeVisible();
  await expect(restart.getByRole("button", { name: "Riprendi comunque" })).toHaveCount(0);
  await restart.getByRole("button", { name: "Riprendi", exact: true }).click();
  await expect(restart.getByText("In corso", { exact: true })).toBeVisible();

  const budget = card(page, BUDGET);
  await expect(budget.getByText("Ha usato tutto il tempo che gli avevi dato", { exact: false })).toBeVisible();
  await expect(budget.getByRole("button", { name: "Riprendi", exact: true })).toHaveCount(0);
  await expect(budget.getByRole("button", { name: "Riprendi comunque" })).toHaveCount(0);
  await expect(budget.getByRole("list", { name: "Istruzioni date" }).getByText("Bloccata", { exact: true })).toBeVisible();
  await expect(budget.getByText("cancellare una prenotazione esistente non rientra in questo lavoro", { exact: false })).toBeVisible();
  await budget.getByRole("button", { name: "Aggiungi 10 minuti" }).click();
  await expect(budget.getByText("In corso", { exact: true })).toBeVisible();
});

test("the instruction box counts the characters and refuses a text that is too long", async ({ page }) => {
  await openInbox(page);
  const run = card(page, RUN);
  const say = run.getByRole("textbox", { name: "Istruzione" });
  const send = run.getByRole("button", { name: "Invia", exact: true });
  await expect(send).toBeDisabled();
  await say.fill("a".repeat(4001));
  await expect(run.getByText(/^4001 \/ 4000$/)).toBeVisible();
  await expect(run.getByRole("alert").filter({ hasText: "L'istruzione supera i 4000 caratteri." })).toBeVisible();
  await expect(send).toBeDisabled();
  await expect(say).toHaveAttribute("aria-invalid", "true");
  await say.fill("a".repeat(4000));
  await expect(send).toBeEnabled();
  await expect(run.getByRole("alert")).toHaveCount(0);
});

test("the overview says how many tasks are at work and paused, and links to them", async ({ page }) => {
  await openApp(page, "/", { mode: "developer" });
  const line = page.getByText("1 lavoro in corso · 3 lavori in pausa");
  await expect(line).toBeVisible();
  await page.getByRole("link", { name: "Guarda i lavori" }).click();
  await expect(page).toHaveURL(/#\/inbox$/);
  await expect(panel(page)).toBeVisible();
});

test("a new installation has no task", async ({ page }) => {
  await openInbox(page, "first-run");
  await expect(panel(page).getByText("Nessun lavoro in background.", { exact: false })).toBeVisible();
  await expect(panel(page).getByRole("article")).toHaveCount(0);
  await expect(panel(page).getByRole("heading", { name: "Finiti" })).toHaveCount(0);
});

test("the keyboard reaches every command of a task", async ({ page }) => {
  await openInbox(page);
  const run = card(page, RUN);
  await run.getByRole("button", { name: "Pausa", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(run.getByText("Si sta fermando", { exact: true })).toBeVisible();
  // Enter adds a line in this box (an instruction can be long); Ctrl+Enter sends.
  const say = run.getByRole("textbox", { name: "Istruzione" });
  await say.fill("Fai con calma.");
  await say.press("Control+Enter");
  await expect(say).toHaveValue("");
  await expect(run.getByRole("list", { name: "Istruzioni date" }).getByText("Fai con calma.")).toBeVisible();
});
