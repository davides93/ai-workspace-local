import type { Locator, Page } from "@playwright/test";
import { expect, openApp, test } from "../support/app";

// Keyboard-only use: the main navigation can be reached with Tab and shows where the focus is,
// dialogs keep the focus inside and close with Esc (giving the focus back), the command palette
// works without the mouse. The navigation tests are of the developer interface (the simple one has no
// section list); the simple one has its own: skip link, menu, message box.

/** The focused element, with what a keyboard user needs to see: a name, and a visible focus indicator. */
async function focused(page: Page) {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    if (!el || el === document.body) return null;
    const cs = getComputedStyle(el);
    const outline = cs.outlineStyle !== "none" && parseFloat(cs.outlineWidth) > 0;
    const ring = cs.boxShadow !== "none";
    const label = el.getAttribute("aria-label") ?? el.textContent ?? "";
    return { name: label.replace(/\s+/g, " ").trim(), tag: el.tagName.toLowerCase(), href: el.getAttribute("href"), visible: outline || ring };
  });
}

/** Presses Tab until `done` says the focus is where we wanted it; fails after `max` presses. Returns every stop. */
async function tabUntil(page: Page, done: (f: NonNullable<Awaited<ReturnType<typeof focused>>>) => boolean, max = 60) {
  const stops: NonNullable<Awaited<ReturnType<typeof focused>>>[] = [];
  for (let i = 0; i < max; i++) {
    await page.keyboard.press("Tab");
    const f = await focused(page);
    if (!f) continue;
    stops.push(f);
    if (done(f)) return stops;
  }
  throw new Error(`The focus never got there in ${max} Tab presses. Stops: ${stops.map((s) => `${s.tag}:${s.name}`).join(" > ")}`);
}

test.describe("keyboard", () => {
  test("the main navigation is reachable with Tab and the focus is visible", async ({ page }) => {
    await openApp(page, "/", { mode: "developer" });
    // First stop: the skip link; then the navigation, in order.
    await page.keyboard.press("Tab");
    expect((await focused(page))?.name).toBe("Vai al contenuto");

    const wanted = ["#/", "#/chat", "#/agents", "#/models", "#/settings"];
    const seen = new Set<string>();
    const stops = await tabUntil(page, (f) => {
      if (f.href) seen.add(f.href);
      return wanted.every((h) => seen.has(h));
    });
    for (const h of wanted) expect(seen, `the link ${h} was reached`).toContain(h);
    const invisible = stops.filter((s) => !s.visible);
    expect(invisible.map((s) => `${s.tag}:${s.name}`), "stops without a visible focus indicator").toEqual([]);

    // Enter follows the focused link.
    await page.getByRole("navigation", { name: "Sezioni" }).getByRole("link", { name: "Impostazioni" }).focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("heading", { name: "Impostazioni", level: 1 })).toBeVisible();
  });

  test("the phone tab bar is reachable with Tab", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await openApp(page, "/", { mode: "developer" });
    const bar = page.getByRole("navigation", { name: "Sezioni" });
    // Developer mode names the chat section «Workspace» (the simple interface calls it «Chat» and has no tab bar).
    await expect(bar.getByRole("link", { name: "Workspace" })).toBeVisible();
    const seen = new Set<string>();
    const stops = await tabUntil(page, (f) => {
      if (f.href) seen.add(f.href);
      return seen.has("#/chat") && seen.has("#/models");
    });
    expect(stops.filter((s) => !s.visible).map((s) => `${s.tag}:${s.name}`)).toEqual([]);
  });

  /** A dialog traps the focus (Tab and Shift+Tab never leave it) and Esc closes it, giving the focus back. */
  async function expectModal(page: Page, trigger: Locator, dialog: Locator) {
    await trigger.focus();
    await trigger.press("Enter");
    await expect(dialog).toBeVisible();
    // The focus moved inside at once.
    await expect.poll(() => dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);
    for (let i = 0; i < 12; i++) {
      await page.keyboard.press("Tab");
      expect(await dialog.evaluate((d) => d.contains(document.activeElement)), `Tab #${i + 1} stays in the dialog`).toBe(true);
    }
    for (let i = 0; i < 12; i++) {
      await page.keyboard.press("Shift+Tab");
      expect(await dialog.evaluate((d) => d.contains(document.activeElement)), `Shift+Tab #${i + 1} stays in the dialog`).toBe(true);
    }
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(trigger).toBeFocused();
  }

  test("a dialog traps the focus and closes with Esc", async ({ page }) => {
    await openApp(page, "/devices", { mode: "developer" });
    await expectModal(page, page.getByRole("button", { name: "Nuovo token per un'app" }), page.getByRole("dialog", { name: "Nuovo token per un'app" }));
  });

  test("the approvals drawer traps the focus and closes with Esc", async ({ page }) => {
    await openApp(page, "/chat");
    await page.getByRole("button", { name: "Nuova", exact: true }).click();
    // The new session resets the composer: wait for it before typing, or the text can be wiped.
    await expect(page.getByRole("heading", { name: "Cosa vuoi fare?" })).toBeVisible();
    await page.getByRole("textbox", { name: "Messaggio" }).fill("Ordina i PDF in Downloads");
    await page.getByRole("button", { name: "Invia" }).click();
    const banner = page.getByRole("button", { name: /1 conferma in attesa/ });
    await expect(banner).toBeVisible();
    await expectModal(page, banner, page.getByRole("dialog", { name: "Conferme in attesa (1)" }));
  });

  test("the command palette opens with Ctrl+K, filters, navigates with the arrows and closes with Esc", async ({ page }) => {
    await openApp(page, "/", { mode: "developer" });
    await page.keyboard.press("Control+k");
    const palette = page.getByRole("dialog", { name: "Cerca e comandi" });
    await expect(palette).toBeVisible();
    const input = palette.getByRole("combobox", { name: "Cerca una sezione o un comando" });
    await expect(input).toBeFocused();

    // Esc closes and gives the focus back.
    await page.keyboard.press("Escape");
    await expect(palette).toBeHidden();

    await page.keyboard.press("Control+k");
    await input.fill("impost");
    const options = palette.getByRole("option");
    await expect(options.first()).toContainText("Impostazioni");
    await expect(options.first()).toHaveAttribute("aria-selected", "true");
    await expect(input).toHaveAttribute("aria-activedescendant", /.+/);

    // Arrows move the selection (wrapping), Enter runs it.
    await input.fill("");
    const count = await options.count();
    expect(count).toBeGreaterThan(2);
    await page.keyboard.press("ArrowDown");
    await expect(options.nth(1)).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("ArrowUp");
    await page.keyboard.press("ArrowUp");
    await expect(options.nth(count - 1)).toHaveAttribute("aria-selected", "true");

    await input.fill("impost");
    await page.keyboard.press("Enter");
    await expect(palette).toBeHidden();
    await expect(page.getByRole("heading", { name: "Impostazioni", level: 1 })).toBeVisible();
  });

  test.describe("simple interface", () => {
    test("desktop: the skip link, the menu and the message box are reachable with Tab, in that order of use", async ({ page }) => {
      await openApp(page, "/chat");
      // The skip link (the message box has the focus on a mouse device, so it is reached directly): Enter moves the focus to the content.
      const skip = page.getByRole("button", { name: "Vai al contenuto" });
      await skip.focus();
      await page.keyboard.press("Enter");
      await expect(page.locator("#main")).toBeFocused();

      // From the skip link, the next stops are the rail: search, then the menu button, which opens the sheet, with its focus trap.
      await skip.focus();
      const rail = page.getByRole("navigation", { name: "Sezioni" });
      const stops = await tabUntil(page, (f) => f.name === "Menu", 80);
      expect(stops.filter((s) => !s.visible).map((s) => `${s.tag}:${s.name}`), "stops without a visible focus indicator").toEqual([]);
      await expectModal(page, rail.getByRole("button", { name: "Menu" }), page.getByRole("dialog", { name: "Menu" }));

      // The message box is one Tab stop of the page.
      await page.locator("#main").focus();
      const box = await tabUntil(page, (f) => f.tag === "textarea", 80);
      expect(box.at(-1)?.name).toBe("Messaggio");
    });

    // The things a person must be able to do without a mouse, on every width: switch to the developer view, change language and
    // theme, stop everything. Desktop has the kill switch and the way to the developer view in the rail; the menu has the rest.
    for (const size of [{ name: "390", width: 390, height: 844 }, { name: "1440", width: 1440, height: 900 }]) {
      test(`${size.name}: kill switch, language, theme and the developer view are reachable with Tab`, async ({ page }) => {
        await page.setViewportSize({ width: size.width, height: size.height });
        await openApp(page, "/chat");
        const wanted = ["Kill switch", "Italiano", "English", "Sistema", "Chiaro", "Scuro", "Semplice", "Sviluppatore"];
        const seen = new Set<string>();

        // Open the menu with the keyboard.
        const menu = page.getByRole("button", { name: "Menu", exact: true });
        await menu.focus();
        await page.keyboard.press("Enter");
        const dialog = page.getByRole("dialog", { name: "Menu" });
        await expect(dialog).toBeVisible();
        const stops = await tabUntil(page, (f) => {
          for (const w of wanted) if (f.name === w || f.name.startsWith(`${w} `) || f.name.startsWith(w)) seen.add(w);
          return wanted.every((w) => seen.has(w));
        }, 40);
        expect([...seen].sort()).toEqual([...wanted].sort());
        expect(stops.filter((s) => !s.visible).map((s) => `${s.tag}:${s.name}`), "stops without a visible focus indicator").toEqual([]);

        // The developer view with the keyboard: the section list appears.
        await dialog.getByRole("group", { name: "Modalità dell'interfaccia" }).getByRole("button", { name: "Sviluppatore" }).focus();
        await page.keyboard.press("Enter");
        await page.keyboard.press("Escape");
        await expect(dialog).toBeHidden();
        const sections = page.getByRole("navigation", { name: "Sezioni" }).getByRole("link", { name: "Runtime" });
        const bar = page.getByRole("navigation", { name: "Sezioni principali" }).getByRole("link", { name: "Runtime" });
        await expect(size.width < 701 ? bar : sections).toBeVisible();
      });
    }

    test("desktop: the kill switch and «Mostra tutte le sezioni» are in the rail, one Tab stop each", async ({ page }) => {
      await openApp(page, "/chat");
      const seen = new Set<string>();
      await tabUntil(page, (f) => {
        seen.add(f.name);
        return seen.has("Kill switch") || [...seen].some((n) => n.startsWith("Kill switch"));
      }, 80);
      expect([...seen].some((n) => n === "Mostra tutte le sezioni")).toBe(true);
    });

    test("the command palette offers the chat, a new session and the download of a model, and the last types the request in the message box", async ({ page }) => {
      await openApp(page, "/chat");
      await page.keyboard.press("Control+k");
      const palette = page.getByRole("dialog", { name: "Cerca e comandi" });
      await expect(palette).toBeVisible();
      const options = palette.getByRole("option");
      await expect(options.filter({ hasText: "Impostazioni" })).toHaveCount(0);
      await expect(options.filter({ hasText: "Modelli" })).toHaveCount(0);
      await expect(options.filter({ hasText: "Nuova sessione" })).toHaveCount(1);
      await palette.getByRole("combobox", { name: "Cerca una sezione o un comando" }).fill("scarica");
      await page.keyboard.press("Enter");
      await expect(palette).toBeHidden();
      await expect(page).toHaveURL(/#\/chat/);
      await expect(page.getByRole("textbox", { name: "Messaggio" })).toHaveValue("Scarica un modello adatto a questo computer");
    });
  });
});
