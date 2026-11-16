import type { Page } from "@playwright/test";
import { expect, openApp, test } from "../support/app";
import { conversation, messageBox } from "../support/simple";

// A conversation of hundreds of rows (scenario `long-chat`): the rows far from the end are not laid out until they come near
// the screen (`content-visibility: auto`, chat.css). The point of these tests is what must NOT change: everything stays in the
// document, so find in page, selection, the screen reader, the keyboard and the automatic scroll still work.
// They check Chromium, which is what Playwright drives here; WebKit and WebView2 were not tried.

const ROWS = ".ws-log-inner > .ws-msg";
/** How many of the replies are laid out (their content is "visible") and how many are skipped by `content-visibility: auto`. */
const skipped = (page: Page) =>
  page.evaluate(() => {
    const bubbles = [...document.querySelectorAll(".ws-log-inner > .ws-msg > .ws-bubble")];
    const off = bubbles.filter((b) => b.firstElementChild && !b.firstElementChild.checkVisibility({ contentVisibilityAuto: true })).length;
    return { total: bubbles.length, skipped: off };
  });
const atEnd = (page: Page) => page.evaluate(() => { const l = document.querySelector(".ws-log")!; return l.scrollHeight - l.scrollTop - l.clientHeight; });
const pill = (page: Page) => page.getByRole("button", { name: "Nuovi messaggi" });

test.describe("long conversation", () => {
  test.beforeEach(async ({ page }) => {
    await openApp(page, "/chat/ses_long", { scenario: "long-chat" });
    await expect(conversation(page)).toContainText("Fine della conversazione");
  });

  test("opens at the end, with the whole conversation in the page and most of it not laid out", async ({ page }) => {
    await expect.poll(() => atEnd(page)).toBeLessThanOrEqual(64);
    const rows = await page.locator(ROWS).count();
    expect(rows).toBeGreaterThan(300);
    const { total, skipped: off } = await skipped(page);
    // The last 40 rows are never skipped; the replies far above are.
    expect(off).toBeGreaterThan(total / 2);
    // Skipped or not, the text of every row is in the document and the log still names itself.
    await expect(conversation(page)).toContainText("Domanda numero 1:");
    await expect(conversation(page).getByText("calibro-quarantadue")).toHaveCount(1);
    expect(await page.locator(".ws-log-inner .codeblock").count()).toBeGreaterThanOrEqual(120);
  });

  test("a reply far from the end is laid out as wide, with the same text width, as one near the end", async ({ page }) => {
    // The padding and the negative margin of the rule must give the same content box (the text wraps in the same places).
    const widths = await page.evaluate(() => {
      const md = [...document.querySelectorAll(".ws-log-inner > .ws-msg > .ws-bubble > .md")];
      return { far: md[0].getBoundingClientRect().width, near: md[md.length - 1].getBoundingClientRect().width, count: md.length };
    });
    expect(widths.count).toBeGreaterThan(100);
    expect(widths.far).toBeGreaterThan(300);
    expect(widths.far).toBeCloseTo(widths.near, 1);
  });

  test("find in page finds a word in the part that is not laid out, and the engine lays that row out for the match", async ({ page }) => {
    const before = await skipped(page);
    expect(before.skipped).toBeGreaterThan(0);
    // The reply with the word is far from the end, so it starts skipped.
    expect(await page.evaluate(() => {
      const bubble = [...document.querySelectorAll(".ws-log-inner > .ws-msg > .ws-bubble")].find((b) => b.textContent?.includes("calibro-quarantadue"));
      return bubble?.firstElementChild?.checkVisibility({ contentVisibilityAuto: true });
    })).toBe(false);
    // `window.find` is the engine's find in page (what Ctrl+F uses), asked from the page.
    expect(await page.evaluate(() => (window as unknown as { find: (s: string, c?: boolean, b?: boolean, w?: boolean) => boolean }).find("calibro-quarantadue", false, false, true))).toBe(true);
    expect(await page.evaluate(() => String(getSelection()))).toBe("calibro-quarantadue");
    // Found, so the engine made the row relevant: it is laid out now (scrolling to the match is the browser's own business
    // and `window.find` does not do it in a headless browser, so that part is not asserted).
    await expect.poll(async () => (await skipped(page)).skipped).toBeLessThan(before.skipped);
    expect(await page.evaluate(() => {
      const node = getSelection()!.getRangeAt(0).startContainer.parentElement!;
      return node.closest(".ws-bubble")!.firstElementChild!.checkVisibility({ contentVisibilityAuto: true });
    })).toBe(true);
  });

  test("a selection across the whole conversation holds all of its text", async ({ page }) => {
    const sel = await page.evaluate(() => {
      const log = document.querySelector(".ws-log")!;
      const r = document.createRange();
      r.selectNodeContents(log);
      const s = getSelection()!;
      s.removeAllRanges();
      s.addRange(r);
      const text = String(s);
      return { len: text.length, inner: (log as HTMLElement).innerText.length, first: text.includes("Domanda numero 1:"), needle: text.includes("calibro-quarantadue"), last: text.includes("Fine della conversazione"), code: text.includes("def top_words") };
    });
    expect(sel.first && sel.needle && sel.last && sel.code).toBe(true);
    // As much text as the page itself reports for the log (a few characters of difference at most: line breaks).
    expect(Math.abs(sel.len - sel.inner) / sel.inner).toBeLessThan(0.02);
  });

  test("scrolling back through the history shows no «Nuovi messaggi», lands on the first question and comes back to the end", async ({ page }) => {
    const log = page.locator(".ws-log");
    for (let i = 0; i < 24; i++) {
      await log.evaluate((el) => el.scrollBy(0, -(el.clientHeight - 80)));
      // Rows come near the screen and take their real height: the list grows or shrinks, and nothing has arrived.
      await expect(pill(page)).toHaveCount(0);
    }
    await log.evaluate((el) => { el.scrollTop = 0; });
    await expect(page.getByText("Domanda numero 1:", { exact: false }).first()).toBeInViewport();
    await expect(pill(page)).toHaveCount(0);
    await log.evaluate((el) => { el.scrollTop = el.scrollHeight; });
    await expect.poll(() => atEnd(page)).toBeLessThanOrEqual(64);
    await expect(pill(page)).toHaveCount(0);
  });

  test("a reply that arrives while the reader is far from the end shows the pill; the pill brings the reader back; at the end the view follows the reply", async ({ page }) => {
    const log = page.locator(".ws-log");
    await messageBox(page).fill("Ordina i PDF in Downloads");
    await page.getByRole("button", { name: "Invia" }).click();
    // At the end, the reply is followed as it grows, up to the confirmation that stops the run.
    await expect(conversation(page).getByRole("article", { name: "shell" })).toBeVisible();
    await expect.poll(() => atEnd(page)).toBeLessThanOrEqual(64);
    // The reader goes back to re-read. The run is parked on the confirmation: nothing arrives until it is answered, so what comes
    // next (the tool's result, the end of the run) comes after the reader has left the end, whatever the speed of the machine.
    await log.evaluate((el) => { el.scrollTop = el.scrollHeight / 2; });
    await expect.poll(() => atEnd(page)).toBeGreaterThan(1000);
    await expect(pill(page)).toHaveCount(0);
    const top = await log.evaluate((el) => el.scrollTop);
    // Answered from the banner's drawer, which does not scroll the log.
    await page.getByRole("button", { name: /1 conferma in attesa/ }).click();
    await page.getByRole("dialog", { name: "Conferme in attesa (1)" }).getByRole("button", { name: "Consenti", exact: true }).click();
    await expect(pill(page)).toBeVisible();
    // The new things did not pull the view: it is where the reader left it, give or take what the rows above resolved to.
    expect(Math.abs((await log.evaluate((el) => el.scrollTop)) - top)).toBeLessThan(400);
    await pill(page).click();
    await expect.poll(() => atEnd(page)).toBeLessThanOrEqual(64);
    await expect(pill(page)).toHaveCount(0);
  });

  test("the keyboard walks back through every control of the conversation, rows not laid out included", async ({ page }) => {
    const total = await page.evaluate(() => {
      const sel = 'button:not([disabled]), a[href], pre[tabindex="0"], summary, textarea, input:not([disabled])';
      const all = [...document.querySelector(".ws-log-inner")!.querySelectorAll<HTMLElement>(sel)];
      (window as unknown as { __all: HTMLElement[] }).__all = all;
      all[all.length - 1].focus();
      return all.length;
    });
    expect(total).toBeGreaterThan(400);
    const index = () => page.evaluate(() => (window as unknown as { __all: HTMLElement[] }).__all.indexOf(document.activeElement as HTMLElement));
    let expected = total - 1;
    expect(await index()).toBe(expected);
    // 180 steps take the focus through more than a quarter of the controls, most of them in rows that were not laid out.
    for (let i = 0; i < 180; i++) {
      await page.keyboard.press("Shift+Tab");
      expected -= 1;
      expect(await index(), `step ${i + 1}`).toBe(expected);
    }
    // The element in focus is on the screen: the browser laid it out and scrolled to it.
    await expect.poll(() => page.evaluate(() => { const r = document.activeElement!.getBoundingClientRect(); return r.bottom > 0 && r.top < window.innerHeight; })).toBe(true);
  });

  test("the code blocks are coloured wherever they are, and the copy button copies the code", async ({ page }) => {
    // The grammars load on demand (the mockup is one file, so it is quick): every language of the scenario gets its colours.
    await expect.poll(() => page.locator(".ws-log-inner .codeblock .tok-kw").count()).toBeGreaterThan(20);
    const langs = await page.evaluate(() => [...document.querySelectorAll(".ws-log-inner pre code[class^='language-']")].map((c) => c.className.replace("language-", "")));
    expect(new Set(langs)).toEqual(new Set(["ts", "python", "rust", "shell", "json", "sql", "css", "diff", "yaml", "go"]));
    // Text is text: what is on the screen is what is in the code (no markup was made out of it).
    const same = await page.evaluate(() => [...document.querySelectorAll(".ws-log-inner pre code")].every((c) => !c.querySelector("script, img, iframe")));
    expect(same).toBe(true);
  });
});
