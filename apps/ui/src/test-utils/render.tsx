import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import { render, screen, waitFor, within, type RenderResult } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Fragment, type ReactElement, type ReactNode } from "react";
import { expect } from "vitest";
import { App } from "../app/App";
import { Providers } from "../app/providers";
import { ApiProvider } from "../shared/api/context";
import { createQueryClient } from "../shared/query/client";
import { useLocale } from "../shared/i18n";
import { MockTransport } from "../shared/api/mock";
import { applyMockScenario, applyMockUiMode, type MockScenario } from "../shared/api/mock/scenario";
import type { UiMode } from "../shared/lib/uiMode";

// The demo data are module state (`shared/api/mock/fixtures`); vitest gives every test file its own copy, so the
// state a test leaves behind (a session created, a model installed) is seen by the next test of the same file:
// write the tests of a file so they do not depend on it, or choose a different session for each.

export interface AppOptions {
  /** Hash route to open (`/chat`, `/settings`, `/models/catalog`). */
  route?: string;
  /** The interface mode the demo configuration says (the default demo starts in simple mode). */
  mode?: UiMode;
  /** Applied once per test file: it empties what a new installation does not have yet (see `scenario.ts`). */
  scenario?: MockScenario;
  /** Another transport, e.g. a `MockTransport` with a method replaced to make a request fail. */
  api?: MockTransport;
}

const applied = new Set<MockScenario>();

/** As in `main.tsx`: a change of language remounts the app, so every text and every Intl format is rendered again. */
function LocaleBoundary({ children }: { children: ReactNode }) {
  return <Fragment key={useLocale()}>{children}</Fragment>;
}

/** `userEvent` with no delay between the steps: the tests wait for the app, not for typing. */
export const setup = () => userEvent.setup({ delay: null });

/**
 * Renders the whole app (shell, route table, the real view, the demo transport) on `route`, as the browser would after
 * `#/route`, and waits until the shell and the view are there. A view is a lazy chunk: the first test of a file pays for it.
 */
export async function renderApp({ route = "/chat", mode = "simple", scenario, api = new MockTransport() }: AppOptions = {}): Promise<RenderResult & { api: MockTransport; user: ReturnType<typeof setup> }> {
  if (scenario && !applied.has(scenario)) {
    applyMockScenario(scenario);
    applied.add(scenario);
  }
  applyMockUiMode(mode);
  location.hash = route;
  const user = setup();
  const result = render(<Providers api={api}><LocaleBoundary><App /></LocaleBoundary></Providers>);
  await screen.findByRole("main");
  // The skeleton of a loading view is a `status` named «Caricamento».
  await waitFor(() => expect(screen.queryByLabelText("Caricamento")).toBeNull(), { timeout: 5000 });
  return Object.assign(result, { api, user });
}

/**
 * Renders one component inside the providers (query client and transport) without the shell. The query client is returned
 * so a test can look into the cache.
 */
export function renderIn(
  ui: ReactElement,
  api: MockTransport = new MockTransport(),
  client: QueryClient = createQueryClient(),
): RenderResult & { user: ReturnType<typeof setup>; client: QueryClient; api: MockTransport } {
  const result = render(<QueryClientProvider client={client}><ApiProvider api={api}>{ui}</ApiProvider></QueryClientProvider>);
  return Object.assign(result, { user: setup(), client, api });
}

export { screen, waitFor, within };
