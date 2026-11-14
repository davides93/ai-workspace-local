import { expect, type Page } from "@playwright/test";

/** An obviously fake key of the shape `sk-…` (built from parts so that no scanner takes it for a credential). */
export const FAKE_KEY = ["sk", "TEST_ONLY_not_a_real_key_0000"].join("-");

/** The `href` of every in-app link on the page (`#/…`): the ones a person could follow. */
export const appLinks = (page: Page): Promise<string[]> =>
  page.evaluate(() => [...document.querySelectorAll("a[href]")].map((a) => a.getAttribute("href") ?? "").filter((h) => h.startsWith("#/")));

/** Links that lead somewhere other than the chat: in the simple interface there must be none, in any state. */
export const linksOutsideChat = (hrefs: string[]): string[] => hrefs.filter((h) => !/^#\/chat(\/|\?|$)/.test(h));

export const messageBox = (page: Page) => page.getByRole("textbox", { name: "Messaggio" });
export const conversation = (page: Page) => page.getByRole("log", { name: "Conversazione" });

/**
 * Starts a download from the catalog of the developer interface (the simple one has no such page and the demo's scripted
 * download does not transfer anything) and goes back to the simple interface through the palette, as a person would:
 * the download goes on in the chat. Ends in the chat with the transfer running.
 */
export async function startDownloadThenGoSimple(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Scarica 256 MB" }).click();
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Cerca e comandi" });
  await palette.getByRole("combobox", { name: "Cerca una sezione o un comando" }).fill("semplice");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/chat/);
}
