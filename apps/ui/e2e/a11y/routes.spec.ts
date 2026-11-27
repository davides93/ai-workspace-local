import { fileURLToPath } from "node:url";
import { collectRoutePaths } from "../../scripts/lib/routes.mjs";
import { expect, openApp, test } from "../support/app";
import { expectNoBlockingViolations, expectNoPlaceholderLeaks } from "../support/scan";

// Every route of the route table (the same list as the smoke test) at phone and desktop width, in the
// light and the dark theme, scanned with axe-core. Violations of impact `critical` or `serious` fail
// the test; the others are attached to the report. See e2e/support/axe.ts for the rules switched off.
// The routes are scanned in developer mode (`?ui=developer`): the simple interface has the chat only, and `openApp`
// checks that each route opened as asked, so that a redirect to the chat cannot make this suite scan the chat 19 times.
// The simple interface has its own tests below: the chat, its menu and an address that redirects.
const root = fileURLToPath(new URL("../..", import.meta.url));
const { paths } = collectRoutePaths(root);

const SIZES = [
  { name: "390", viewport: { width: 390, height: 844 } },
  { name: "1440", viewport: { width: 1440, height: 900 } },
] as const;

for (const size of SIZES) {
  for (const theme of ["light", "dark"] as const) {
    test.describe(`${size.name} · ${theme}`, () => {
      test.use({ viewport: size.viewport, colorScheme: theme });
      for (const route of paths) {
        test(route, async ({ page }, info) => {
          await openApp(page, route, { theme, mode: "developer" });
          await expectNoBlockingViolations(page, info);
          await expectNoPlaceholderLeaks(page);
        });
      }
    });

    test.describe(`${size.name} · ${theme} · simple interface`, () => {
      test.use({ viewport: size.viewport, colorScheme: theme });
      test("/chat", async ({ page }, info) => {
        await openApp(page, "/chat", { theme });
        await expect(page.getByRole("textbox", { name: "Messaggio" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });
      test("the menu", async ({ page }, info) => {
        await openApp(page, "/chat", { theme });
        await page.getByRole("button", { name: "Altro", exact: true }).click();
        await expect(page.getByRole("dialog", { name: "Altro" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });
      test("an address that redirects to the chat", async ({ page }, info) => {
        await openApp(page, "/runtimes", { theme });
        await expect(page).toHaveURL(/#\/chat/);
        await expect(page.getByRole("textbox", { name: "Messaggio" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });
    });
  }
}
