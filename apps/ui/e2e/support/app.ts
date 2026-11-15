import { expect, test as base, type Page } from "@playwright/test";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
export const mockupPath = path.resolve(process.env.AIW_E2E_HTML ?? path.join(root, "dist-mockup", "index.html"));

export interface OpenOptions {
  theme?: "light" | "dark";
  /**
   * `developer` shows every section (the demo starts with `?ui=developer`); the default is the simple interface of a new
   * installation, which has the chat only: any other route goes to it.
   */
  mode?: "simple" | "developer";
  /** Starting state of the demo data (`?scenario=`, see src/shared/api/mock/scenario.ts). */
  scenario?: "first-run" | "mas";
}

/** Path of a hash route without its query (`/models/discover?q=x` → `/models/discover`). */
const pathOf = (route: string) => route.split("?")[0].replace(/\/+$/, "") || "/";

/**
 * Opens the mockup on `route` (a hash route such as `/chat`) with the given preferences stored on the device. In developer
 * mode it also checks that the route was opened as asked: a wrong redirect to the chat would otherwise let a test of a
 * section pass on the chat (the sessions of the chat count as the chat: `/chat` → `/chat/ses_…`).
 */
export async function openApp(page: Page, route = "/", opts: OpenOptions = {}): Promise<void> {
  if (!existsSync(mockupPath)) throw new Error(`Missing ${mockupPath}: run "npm run build:mockup" first.`);
  const { theme = "light", mode = "simple", scenario } = opts;
  // The language follows the browser locale of the test (`test.use({ locale: "en-US" })`): the
  // configuration says `auto`, and that wins over anything kept on the device.
  await page.addInitScript((t) => {
    try {
      localStorage.setItem("aiw.theme", t);
    } catch {
      /* storage unavailable */
    }
  }, theme);
  const url = new URL(pathToFileURL(mockupPath).href);
  if (scenario) url.searchParams.set("scenario", scenario);
  // The interface mode lives in the configuration, and the demo starts in simple mode: `?ui=developer` makes the configuration
  // say developer. Nothing is stored on the device on purpose, like a person who opens the app on a new device.
  if (mode === "developer") url.searchParams.set("ui", "developer");
  url.hash = route;
  await page.goto(url.href);
  // The app is up when its main area is there (on a phone the simple interface has neither rail nor tab bar) and no view is still loading.
  await expect(page.locator("#main")).toBeVisible();
  await expect(page.getByLabel(/^(Caricamento|Loading)$/)).toHaveCount(0);
  if (mode === "developer") {
    const want = pathOf(route);
    await expect.poll(() => new URL(page.url()).hash.replace(/^#/, "").split("?")[0], { message: `developer mode opens ${want} as asked` }).toMatch(new RegExp(`^${want.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(/|$)`));
  }
}

/**
 * `test` with two guards every flow gets for free: no console error and no uncaught exception
 * (the mockup is local-first, so no network request either).
 */
export const test = base.extend<{ problems: string[] }>({
  problems: [
    async ({ page }, use) => {
      const problems: string[] = [];
      page.on("console", (m) => {
        if (m.type() === "error") problems.push(`console: ${m.text()}`);
      });
      page.on("pageerror", (e) => problems.push(`uncaught: ${e.message}`));
      page.on("request", (r) => {
        if (/^https?:/.test(r.url())) problems.push(`network request to ${r.url()}`);
      });
      await use(problems);
      expect(problems, "console errors, uncaught exceptions or network requests").toEqual([]);
    },
    { auto: true },
  ],
});

export { expect };
