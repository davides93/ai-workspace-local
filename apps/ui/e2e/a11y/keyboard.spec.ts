import type { Locator, Page } from "@playwright/test";
import { expect, openApp, test } from "../support/app";

// Keyboard-only use: the main navigation can be reached with Tab and shows where the focus is,
// dialogs keep the focus inside and close with Esc (giving the focus back), the command palette
// works without the mouse.

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
    await openApp(page, "/");
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
    await openApp(page, "/");
    const bar = page.getByRole("navigation", { name: "Sezioni" });
    await expect(bar.getByRole("link", { name: "Chat" })).toBeVisible();
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
    await page.getByRole("textbox", { name: "Messaggio" }).fill("Ordina i PDF in Downloads");
    await page.getByRole("button", { name: "Invia" }).click();
    const banner = page.getByRole("button", { name: /1 conferma in attesa/ });
    await expect(banner).toBeVisible();
    await expectModal(page, banner, page.getByRole("dialog", { name: "Conferme in attesa (1)" }));
  });

  test("the command palette opens with Ctrl+K, filters, navigates with the arrows and closes with Esc", async ({ page }) => {
    await openApp(page, "/");
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
});
