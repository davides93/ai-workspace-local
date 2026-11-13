import { fileURLToPath } from "node:url";
import { collectRoutePaths } from "../../scripts/lib/routes.mjs";
import { openApp, test } from "../support/app";
import { expectNoBlockingViolations, expectNoPlaceholderLeaks } from "../support/scan";

// Every route of the route table (the same list as the smoke test) at phone and desktop width, in the
// light and the dark theme, scanned with axe-core. Violations of impact `critical` or `serious` fail
// the test; the others are attached to the report. See e2e/support/axe.ts for the rules switched off.
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
          await openApp(page, route, { theme });
          await expectNoBlockingViolations(page, info);
          await expectNoPlaceholderLeaks(page);
        });
      }
    });
  }
}
