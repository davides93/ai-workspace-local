import { describe, expect, it } from "vitest";
import { renderApp, screen } from "../test-utils/render";

describe("App in jsdom", () => {
  it("opens the chat of the simple interface", async () => {
    await renderApp({ route: "/chat" });
    expect(screen.getByRole("main")).toBeTruthy();
    expect(location.hash).toMatch(/^#\/chat/);
  });
});
