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

/**
 * The page must not scroll sideways: neither the document nor the app's own scroll area is wider than its window, and
 * nothing inside `within` (a selector; the whole main area when omitted) reaches past the right edge of the window.
 */
export async function expectNoHorizontalScroll(page: Page, within = "#main"): Promise<void> {
  const wide = await page.evaluate((root) => {
    const out: string[] = [];
    const name = (el: Element) => `${el.tagName.toLowerCase()}${el.id ? `#${el.id}` : ""}${typeof el.className === "string" && el.className ? `.${el.className.split(" ")[0]}` : ""}`;
    for (const el of [document.documentElement, document.body, document.querySelector("#main"), document.querySelector(".page")]) {
      if (el && el.scrollWidth > el.clientWidth) out.push(`${name(el)} scrolls sideways: ${el.scrollWidth} > ${el.clientWidth}`);
    }
    const edge = window.innerWidth;
    // What an ancestor clips (a line clamp, a scrolling code block) does not widen the page.
    const clipped = (el: Element) => {
      for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
        if (getComputedStyle(p).overflowX !== "visible" && p.getBoundingClientRect().right <= edge + 1) return true;
      }
      return false;
    };
    for (const el of document.querySelectorAll(`${root} *`)) {
      const r = el.getBoundingClientRect();
      if (r.width > 1 && r.right > edge + 1 && getComputedStyle(el).position !== "fixed" && !clipped(el)) out.push(`${name(el)} ends at ${Math.round(r.right)} of ${edge}`);
    }
    return out.slice(0, 10);
  }, within);
  expect(wide, "nothing may reach past the right edge of the window").toEqual([]);
}

/** A translated text that still shows a `{placeholder}` means a call and its message disagree. */
export async function expectNoPlaceholderLeaks(page: Page): Promise<void> {
  const leaks = await page.evaluate(() => {
    const out: string[] = [];
    // `{{now}}` (a workflow placeholder shown as an example) is not a leak.
    const bad = /(?<!\{)\{\w+\}(?!\})/;
    // A template written on purpose inside a code sample (`/agui/{agente}` in the guide of Integrazioni) is not a leak either:
    // the code samples are hidden for the scan of the text around them, then shown again.
    const samples = [...document.querySelectorAll<HTMLElement>("code, pre")].map((el) => [el, el.style.display] as const);
    for (const [el] of samples) el.style.display = "none";
    const text = document.body.innerText.match(bad)?.[0];
    for (const [el, display] of samples) el.style.display = display;
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
