import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SecretWarning } from "./SecretWarning";

// The markup of the warning (the project has no DOM test environment; the behaviour is in the end-to-end tests).
// An obviously fake key, built from parts so that no scanner takes it for a credential.
const FAKE_KEY = ["sk", "TEST_ONLY_not_a_real_key_0000"].join("-");
const BUTTON = "Togli il testo e chiedi il campo protetto";
const WARNING = "Sembra una chiave o un token";

const render = (text: string, onAskProtectedField?: () => void) =>
  renderToStaticMarkup(createElement(SecretWarning, { text, onAskProtectedField }));

describe("SecretWarning", () => {
  it("a secret: the warning and, when the agent can ask for the protected field, the button", () => {
    const html = render(`Usa ${FAKE_KEY}`, () => {});
    expect(html).toContain(WARNING);
    expect(html).toContain(BUTTON);
  });
  it("a secret and an agent that cannot ask for the field: the warning stays, the button does not come", () => {
    const html = render(`Usa ${FAKE_KEY}`);
    expect(html).toContain(WARNING);
    expect(html).toContain("Chiedi all&#x27;assistente di usare il campo protetto");
    expect(html).not.toContain(BUTTON);
    expect(html).not.toContain("<button");
  });
  it("an ordinary text: only the live region, empty", () => {
    for (const onAsk of [undefined, () => {}]) {
      const html = render("Riordina i PDF in Downloads", onAsk);
      expect(html).toContain('role="status"');
      expect(html).not.toContain(WARNING);
      expect(html).not.toContain("<button");
    }
  });
});
