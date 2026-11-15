import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { ApiProvider } from "../../../shared/api/context";
import { MockTransport } from "../../../shared/api/mock";
import { SimpleFirstRunPanel } from "./SimpleFirstRun";

// The markup of the panel (the project has no DOM test environment; the behaviour is in the end-to-end tests).
const render = (el: ReactElement) =>
  renderToStaticMarkup(createElement(QueryClientProvider, { client: new QueryClient() }, createElement(ApiProvider, { api: new MockTransport(), children: el })));

describe("SimpleFirstRunPanel, no model of the catalog fits", () => {
  const html = render(createElement(SimpleFirstRunPanel, { state: { kind: "no-model" }, models: [], runtimes: [] }));
  it("the sentence names the button below it: «Mostra tutte le sezioni»", () => {
    expect(html).toContain("con «Mostra tutte le sezioni» puoi cercarne uno a mano.");
    expect(html).toContain("Mostra tutte le sezioni</button>");
  });
});
