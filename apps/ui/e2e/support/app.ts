import { expect, test as base, type Page } from "@playwright/test";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
export const mockupPath = path.resolve(process.env.AIW_E2E_HTML ?? path.join(root, "dist-mockup", "index.html"));

export interface OpenOptions {
  theme?: "light" | "dark";
  /** `developer` shows every section; the default is the simple interface of a new installation. */
  mode?: "simple" | "developer";
  /** Starting state of the demo data (`?scenario=`, see src/shared/api/mock/scenario.ts). */
  scenario?: "first-run";
}

/** Opens the mockup on `route` (a hash route such as `/chat`) with the given preferences stored on the device. */
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
  // The interface mode lives in the configuration (the demo starts in simple mode): a developer run
  // switches it the way a person does, in Settings, then goes to the route.
  url.hash = mode === "developer" ? "/settings" : route;
  await page.goto(url.href);
  // Desktop: the rail ("Sezioni"); phone: the tab bar ("Sezioni principali").
  await expect(page.getByRole("navigation").first()).toBeVisible();
  if (mode === "developer") {
    await page.getByRole("group", { name: "Modalità dell'interfaccia" }).getByRole("button", { name: "Sviluppatore" }).click();
    await expect(page.getByRole("group", { name: "Modalità dell'interfaccia" }).getByRole("button", { name: "Sviluppatore" })).toHaveAttribute("aria-pressed", "true");
    await page.evaluate((r) => { location.hash = r; }, route);
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
