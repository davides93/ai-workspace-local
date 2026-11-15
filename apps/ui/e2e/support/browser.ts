import { expect, type Locator, type Page } from "@playwright/test";
import { chatWith } from "./ask";
import type { OpenOptions } from "./app";

// The controlled browser in the chat of the demo: the session of the Navigatore opens a page (and leaves the browser open), and the phrase
// «Accedi al sito dei fornitori» makes the agent ask the person to log in (a request for help in the browser). See src/shared/api/mock/browser.ts.

/** The conversation with the Navigatore of the demo (`ses_cv`): a message there makes it open a page. */
export const NAVIGATOR_SESSION = "ses_cv";
export const HANDOVER_PHRASE = "Accedi al sito dei fornitori";
/** The title of the card of the request, in the language of the interface. */
export const CARD_TITLE = /^(Serve il tuo intervento nel browser|Your help is needed in the browser)$/;

export const log = (page: Page) => page.getByRole("log", { name: /^(Conversazione|Conversation)$/ });
export const header = (page: Page) => page.locator("header.ws-head");
/** The «Browser» button of the chat header: its name is «Browser: <who drives it>» (the cards of the tool calls say «Browser: browser_navigate…», so it is looked up in the header). */
export const browserButton = (page: Page) => header(page).getByRole("button", { name: /^Browser: / });
/** The panel of the live view: a column on a wide screen (a landmark), a dialog on a phone or a tablet. */
export const panel = (page: Page): Locator =>
  page.getByRole("complementary", { name: /^(Vista del browser|Browser view)$/ }).or(page.getByRole("dialog", { name: /^(Vista del browser|Browser view)$/ }));
/** The card of the request in the conversation: a group (not a form: the field that types into the page must not sit in one). */
export const handoverCard = (page: Page) => log(page).getByRole("group", { name: CARD_TITLE });
/** The viewer while the person drives it. */
export const viewer = (scope: Locator) => scope.getByRole("application", { name: /^Pagina del browser, / });
export const frameImage = (scope: Locator) => scope.locator(".bl-viewer img");

/** Opens the chat and asks the Navigatore to look at a page: the run opens it and leaves the browser open. Ends when the run is over. */
export async function openBrowserSession(page: Page, opts: OpenOptions = {}): Promise<void> {
  await chatWith(page, "Guarda i preventivi dei fornitori", { ...opts, session: NAVIGATOR_SESSION });
  await expect(browserButton(page)).toBeVisible();
  await expect(page.getByRole("button", { name: /^(Ferma|Stop)$/ })).toBeHidden();
}

/** Opens the chat and starts the script in which the agent asks the person to log in: ends when its card is there. */
export async function openHandover(page: Page, opts: OpenOptions & { query?: string } = {}): Promise<void> {
  await chatWith(page, HANDOVER_PHRASE, { ...opts, session: NAVIGATOR_SESSION });
  await expect(handoverCard(page)).toBeVisible();
}

/** What the frame shows as text: the demo draws it as an SVG, one dot per character typed in a field (never the character). */
export async function frameSvg(scope: Locator): Promise<string> {
  const src = await frameImage(scope).getAttribute("src");
  if (!src?.startsWith("data:image/svg+xml;utf8,")) throw new Error("the frame is not the SVG of the demo");
  return decodeURIComponent(src.slice("data:image/svg+xml;utf8,".length));
}

/** The lengths of the rows of dots in the frame: one row a field, one dot a character typed in it (24 at most). */
export async function dots(scope: Locator): Promise<number[]> {
  return ((await frameSvg(scope)).match(/•+/g) ?? []).map((run) => run.length);
}

/** Clicks on the frame at a fraction of its box (the viewer takes it as a click on the page). */
export async function clickFrame(scope: Locator, x: number, y: number): Promise<void> {
  const img = frameImage(scope);
  const box = await img.boundingBox();
  if (!box) throw new Error("the frame has no box");
  await img.click({ position: { x: box.width * x, y: box.height * y } });
}

/** The status line (a polite live region) of the view. */
export const statusLine = (scope: Locator, text: string | RegExp) => scope.getByRole("status").filter({ hasText: text });

/** Signs in on the fake page through the viewer of `scope` (the panel or the card), so that «Esci» can ask its question. Leaves the page on the members' area. */
export async function signInThroughViewer(page: Page, scope: Locator): Promise<void> {
  await clickFrame(scope, 0.5, 0.35);
  await page.keyboard.type("anna@example.com");
  await scope.getByRole("button", { name: "Tab", exact: true }).click();
  await scope.getByLabel("Scrivi nella pagina").fill("TEST-ONLY-pw");
  await scope.getByRole("button", { name: "Invia" }).click();
  await scope.getByRole("button", { name: "Invio" }).click();
  await expect.poll(async () => (await frameSvg(scope)).includes("Area riservata")).toBe(true);
}

/** The transport of the demo as the page uses it (a method of it can be replaced to count, delay or refuse a call). */
export type DemoApi = Record<string, (...args: never[]) => unknown> & { [method: string]: any }; // eslint-disable-line @typescript-eslint/no-explicit-any

/**
 * Runs `fn(api, arg)` in the page with the transport of the demo (found in the React tree, like `whereIs` does). `fn` is serialised: it
 * cannot use anything of this file. What it returns must be serialisable; `window` is the page's.
 */
export function withApi<A, R>(page: Page, fn: (api: DemoApi, arg: A) => R | Promise<R>, arg?: A): Promise<R> {
  return page.evaluate(
    async ({ src, a }) => {
      const container = document.getElementById("root") as unknown as Record<string, unknown>;
      const key = Object.keys(container).find((k) => k.startsWith("__reactContainer$"));
      if (!key) throw new Error("React root not found");
      type Fiber = { memoizedProps?: Record<string, unknown>; child?: Fiber; sibling?: Fiber };
      const stack: Fiber[] = [container[key] as Fiber];
      let api: unknown;
      while (stack.length > 0 && !api) {
        const f = stack.pop()!;
        const candidate = f.memoizedProps?.api as { browserInput?: unknown } | undefined;
        if (typeof candidate?.browserInput === "function") api = candidate;
        if (f.sibling) stack.push(f.sibling);
        if (f.child) stack.push(f.child);
      }
      if (!api) throw new Error("the transport was not found in the React tree");
      return await (new Function("api", "arg", `return (${src})(api, arg);`) as (api: unknown, arg: unknown) => unknown)(api, a);
    },
    { src: fn.toString(), a: arg },
  ) as Promise<R>;
}
