import type { CDPSession, Page } from "@playwright/test";
import { expect, openApp, test } from "../support/app";
import { conversation } from "../support/simple";

// A conversation of hundreds of rows (scenario `long-chat`): the rows far from the end are not laid out until they come near the
// screen (`content-visibility: auto`, chat.css). What a screen reader gets must not change. Chromium builds the full accessibility
// tree only when an assistive technology is attached; the flag below is how a test turns that mode on. The same page is read with
// `content-visibility` forced off and as shipped. Not tried: a real screen reader (NVDA, JAWS, VoiceOver, Orca), WebKit, WebView2.
test.use({ launchOptions: { args: ["--force-renderer-accessibility"], executablePath: process.env.AIW_CHROMIUM || undefined } });

const skipped = (page: Page) =>
  page.evaluate(() => {
    const bubbles = [...document.querySelectorAll(".ws-log-inner > .ws-msg > .ws-bubble")];
    const off = bubbles.filter((b) => b.firstElementChild && !b.firstElementChild.checkVisibility({ contentVisibilityAuto: true })).length;
    return { total: bubbles.length, skipped: off };
  });

test.describe("long conversation and the accessibility tree", () => {
  async function roles(page: Page, cdp: CDPSession): Promise<{ roles: Map<string, number>; text: string; names: string[] }> {
    const { nodes } = (await cdp.send("Accessibility.getFullAXTree")) as { nodes: { ignored?: boolean; role?: { value: string }; name?: { value?: string } }[] };
    void page;
    const map = new Map<string, number>();
    let text = "";
    const names: string[] = [];
    for (const n of nodes) {
      if (n.ignored) continue;
      const role = n.role?.value ?? "";
      map.set(role, (map.get(role) ?? 0) + 1);
      if (role === "StaticText") text += n.name?.value ?? "";
      if (role === "button" && n.name?.value) names.push(n.name.value);
    }
    return { roles: map, text, names };
  }

  test("the replies that are not laid out keep their text, code, tables, lists, headings and buttons for assistive technology", async ({ page }) => {
    await openApp(page, "/chat/ses_long", { scenario: "long-chat" });
    await expect(conversation(page)).toContainText("Fine della conversazione");
    // Syntax colours split a block into many text runs; wait until they are all there, in both readings.
    await expect.poll(() => page.locator(".ws-log-inner .codeblock .tok-kw").count()).toBeGreaterThan(20);
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Accessibility.enable");

    await page.addStyleTag({ content: ".ws-log-inner .ws-bubble { content-visibility: visible !important; }" });
    await expect.poll(async () => (await skipped(page)).skipped).toBe(0);
    const full = await roles(page, cdp);

    await page.reload();
    await page.evaluate(() => { location.hash = "/chat/ses_long"; });
    await expect(conversation(page)).toContainText("Fine della conversazione");
    await expect.poll(() => page.locator(".ws-log-inner .codeblock .tok-kw").count()).toBeGreaterThan(20);
    expect((await skipped(page)).skipped).toBeGreaterThan(50);
    const shipped = await roles(page, cdp);

    // The same words, line for line.
    expect(shipped.text.length).toBe(full.text.length);
    expect(shipped.text).toContain("La risposta al turno 1 usa");
    expect(shipped.text).toContain("def top_words");
    // The same structure and the same controls: every role that carries meaning is as frequent as with everything laid out.
    for (const role of ["heading", "list", "listitem", "table", "row", "cell", "columnheader", "code", "strong", "button", "log", "main", "navigation"]) {
      expect(shipped.roles.get(role) ?? 0, role).toBe(full.roles.get(role) ?? 0);
    }
    expect(shipped.names.filter((n) => /^Copia/.test(n)).length).toBe(full.names.filter((n) => /^Copia/.test(n)).length);
  });
});
