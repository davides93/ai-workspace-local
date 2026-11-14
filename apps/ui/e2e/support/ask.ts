import { expect, type Page } from "@playwright/test";
import { openApp, type OpenOptions } from "./app";

/** An obviously fake Hugging Face token (the demo accepts anything that starts with `hf_`): any trace of it anywhere is a leak. */
export const FAKE_TOKEN = "hf_TEST_ONLY_not_a_real_token_0000";
/** A value the demo refuses (it does not start with `hf_`). */
export const WRONG_TOKEN = "not-a-token-TEST-ONLY-9999";

/**
 * Opens the chat and sends `text` with Enter, so the focus stays in the message box (what a person who types does). On a fresh
 * session by default (the «Nuova» button is in the sessions list: not on a phone, where `session` opens one that exists).
 * Works in Italian and in English; `query` goes in the address of the demo (`?scope=device`).
 */
export async function chatWith(page: Page, text: string, opts: OpenOptions & { query?: string; session?: string } = {}): Promise<void> {
  const { query, session, ...open } = opts;
  if (query) {
    // `openApp` builds the address itself and only knows `?scenario=`: add the query to the one it opens (a file: page cannot change its own query).
    const goto = page.goto.bind(page);
    page.goto = ((url: string, options?: Parameters<Page["goto"]>[1]) => goto(url.replace("#", `?${query}#`), options)) as Page["goto"];
  }
  await openApp(page, session ? `/chat/${session}` : "/chat", open);
  if (session) {
    await expect(page.getByLabel(/^(Caricamento|Loading)$/)).toHaveCount(0);
    await expect(page.getByRole("log", { name: /^(Conversazione|Conversation)$/ })).toBeVisible();
  } else {
    await page.getByRole("button", { name: /^(Nuova|New)$/ }).first().click();
    await expect(page.getByRole("heading", { name: /^(Cosa facciamo, in locale\?|What shall we do, locally\?)$/ })).toBeVisible();
  }
  const box = page.locator("#composer");
  await expect(box).toBeEnabled();
  await box.fill(text);
  await box.press("Enter");
}

/**
 * Looks for each value in everything a page keeps: its markup and text, the value of every field, the storage of the
 * browser, the query and mutation caches of the app and — in the demo — every stored message, run event, audit row and open
 * request. Returns where it was found (empty when nowhere). It fails loudly when the caches or the demo cannot be reached,
 * so that "found nothing" cannot come from not looking.
 */
export async function whereIs(page: Page, needles: string[]): Promise<string[]> {
  return page.evaluate(async (values) => {
    const found: string[] = [];
    const check = (where: string, text: string) => {
      if (values.some((v) => text.includes(v))) found.push(where);
    };
    check("markup", document.documentElement.outerHTML);
    check("text", document.body.innerText);
    check("field value", [...document.querySelectorAll("input, textarea, select")].map((el) => (el as HTMLInputElement).value).join("\n"));
    for (const [name, store] of [["localStorage", localStorage], ["sessionStorage", sessionStorage]] as const) {
      check(name, JSON.stringify(Object.fromEntries(Object.entries(store))));
    }

    // The React tree holds the QueryClient and the transport in the props of their providers.
    const container = document.getElementById("root") as unknown as Record<string, unknown>;
    const key = Object.keys(container).find((k) => k.startsWith("__reactContainer$"));
    if (!key) throw new Error("React root not found");
    type Fiber = { memoizedProps?: Record<string, unknown>; child?: Fiber; sibling?: Fiber };
    const find = <T>(pick: (p: Record<string, unknown>) => T | undefined): T => {
      const stack: Fiber[] = [container[key] as Fiber];
      while (stack.length > 0) {
        const f = stack.pop()!;
        const hit = f.memoizedProps ? pick(f.memoizedProps) : undefined;
        if (hit !== undefined) return hit;
        if (f.sibling) stack.push(f.sibling);
        if (f.child) stack.push(f.child);
      }
      throw new Error("provider not found in the React tree");
    };
    const client = find((p) => (typeof (p.client as { getQueryCache?: unknown } | undefined)?.getQueryCache === "function" ? (p.client as Record<string, () => { getAll(): unknown[] }>) : undefined));
    const api = find((p) => (typeof (p.api as { messages?: unknown } | undefined)?.messages === "function" ? (p.api as Record<string, (...a: string[]) => Promise<unknown>>) : undefined));

    const queries = client.getQueryCache().getAll() as { queryKey: unknown; state: { data: unknown; error: unknown } }[];
    check("query cache", JSON.stringify(queries.map((q) => ({ key: q.queryKey, data: q.state.data, error: String(q.state.error ?? "") }))));
    const mutations = client.getMutationCache().getAll() as { state: { variables: unknown; data: unknown; error: unknown } }[];
    check("mutation cache", JSON.stringify(mutations.map((m) => ({ variables: m.state.variables, data: m.state.data, error: String(m.state.error ?? "") }))));

    const sessions = (await api.sessions()) as { id: string }[];
    const runs = [...new Set((JSON.parse(sessionStorage.getItem("aiw.runs") ?? "[]") as [string, string][]).map(([run]) => run))];
    const stored = await Promise.all([
      ...sessions.map((s) => api.messages(s.id)),
      ...runs.map((r) => api.runEvents(r)),
      sessions, api.audit(), api.secretRequests(), api.questions(), api.approvals(), api.modelProviders(),
    ]);
    check("demo data (messages, events, audit, requests)", JSON.stringify(stored));
    if (runs.length === 0) throw new Error("no run in the session storage: the events of the demo were not looked at");
    return found;
  }, needles);
}
