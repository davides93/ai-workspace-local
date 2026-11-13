import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, type TestInfo } from "@playwright/test";
import { DISABLED_RULES, formatViolations, isBlocking } from "./axe";

/** Waits for a settled page, runs axe-core and fails on violations of impact `critical` or `serious`. */
export async function expectNoBlockingViolations(page: Page, info: TestInfo): Promise<void> {
  // Settled page: lazy view loaded, fonts in, no transition half way (it would skew the contrast).
  await expect(page.getByLabel(/^(Caricamento|Loading)$/)).toHaveCount(0);
  await page.addStyleTag({ content: "*,*::before,*::after{transition:none!important;animation:none!important}" });
  await page.evaluate(() => document.fonts.ready);

  const result = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "best-practice"])
    .disableRules(DISABLED_RULES.map((r) => r.id))
    .analyze();
  const others = result.violations.filter((v) => !isBlocking(v));
  if (others.length > 0) await info.attach("axe-minor-and-moderate.txt", { body: formatViolations(others), contentType: "text/plain" });
  const blocking = result.violations.filter(isBlocking);
  expect(blocking, formatViolations(blocking)).toEqual([]);
}

/** A translated text that still shows a `{placeholder}` means a call and its message disagree. */
export async function expectNoPlaceholderLeaks(page: Page): Promise<void> {
  const leaks = await page.evaluate(() => {
    const out: string[] = [];
    // `{{now}}` (a workflow placeholder shown as an example) is not a leak.
    const bad = /(?<!\{)\{\w+\}(?!\})/;
    const text = document.body.innerText.match(bad)?.[0];
    if (text) out.push(`text: ${text}`);
    for (const el of document.querySelectorAll("[aria-label],[title],[placeholder],[alt]")) {
      for (const a of ["aria-label", "title", "placeholder", "alt"]) {
        const v = el.getAttribute(a);
        if (v && bad.test(v)) out.push(`${a}="${v}"`);
      }
    }
    return out;
  });
  expect(leaks, "unresolved {placeholders} in the visible texts or attributes").toEqual([]);
}
