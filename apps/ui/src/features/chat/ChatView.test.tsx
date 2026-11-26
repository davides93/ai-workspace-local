import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderApp } from "../../test-utils/render";

// The chat view inside the whole shell, against the demo transport, in both interface modes. What the browser does with the layout
// (scrolling, the phone sheet) is in the Playwright tests; here the structure, the rules of each mode and the run of a message.
const SLOW = 30_000;
const log = () => screen.getByRole("log", { name: "Conversazione" });
const box = () => screen.getByRole("textbox", { name: "Messaggio" });

describe("chat, simple interface", () => {
  it("is the Assistente's chat and nothing else: a list of chats, the conversation, the message box, no section links", async () => {
    await renderApp({ route: "/chat", mode: "simple" });
    // The address of the last chat is filled in, like the browser would.
    await waitFor(() => expect(location.hash).toBe("#/chat/ses_pdf"));
    expect(within(log()).getByText(/Quanti PDF ci sono in Downloads/)).toBeInTheDocument();
    expect(box()).toBeEnabled();
    expect(screen.getByRole("button", { name: "Invia" })).toBeInTheDocument();
    expect(within(screen.getByRole("navigation", { name: "Sezioni" })).queryAllByRole("link")).toHaveLength(0);
    expect(screen.getByRole("navigation", { name: "Elenco delle sessioni" })).toBeInTheDocument();
    // Nothing technical to choose: no agent, no engine, no «Prompt MCP».
    expect(screen.queryByLabelText("Agente della nuova sessione")).toBeNull();
    expect(screen.queryByRole("button", { name: "Prompt MCP" })).toBeNull();
    // Every link of the page leads to a chat.
    const hrefs = [...document.querySelectorAll("a[href]")].map((a) => a.getAttribute("href") ?? "").filter((h) => h.startsWith("#/"));
    expect(hrefs.filter((h) => !/^#\/chat(\/|\?|$)/.test(h))).toEqual([]);
  });

  it("an address of another section leads to the chat, and the settings page is not shown", async () => {
    await renderApp({ route: "/settings", mode: "simple" });
    await waitFor(() => expect(location.hash).toMatch(/^#\/chat/));
    expect(screen.queryByRole("heading", { name: "Impostazioni", level: 1 })).toBeNull();
    expect(box()).toBeInTheDocument();
  });

  it("a new chat is the Assistente's and says how it works in words: the approval mode, no sandbox, the model as text", async () => {
    const { user } = await renderApp({ route: "/chat/ses_pdf", mode: "simple" });
    await user.click(screen.getByRole("button", { name: "Nuova" }));
    await screen.findByRole("heading", { name: "Cosa vuoi fare?" });
    expect(within(screen.getByRole("navigation", { name: "Elenco delle sessioni" })).getByRole("button", { name: /^Nuova sessione.*Assistente/ })).toBeInTheDocument();
    const meta = screen.getByRole("list", { name: "Come lavora questa sessione" });
    expect(meta).toHaveTextContent("Approvazione: Chiedi");
    expect(within(meta).queryByText("Sandbox")).toBeNull();
    expect(within(meta).queryAllByRole("link")).toHaveLength(0);
    // It starts from tasks of everyday life: a card puts its sentence in the message box without sending it.
    const ideas = screen.getByRole("list", { name: "Idee per iniziare" });
    expect(within(ideas).getAllByRole("button")).toHaveLength(6);
    await user.click(within(ideas).getByRole("button", { name: /Prepara la mia giornata/ }));
    expect(box()).toHaveValue("Guarda il mio calendario di oggi e preparami un piano della giornata.");
  });

  it("a failed run ends the history with a note that offers to ask the Assistente, not the runtime page", async () => {
    await renderApp({ route: "/chat/ses_bug", mode: "simple" });
    expect(await within(log()).findByText(/memoria video esaurita/)).toBeInTheDocument();
    expect(within(log()).getByRole("button", { name: /^Chiedi all'assistente/ })).toBeInTheDocument();
    expect(within(log()).queryByRole("link", { name: /Apri i runtime/ })).toBeNull();
  });

  it("a message starts a run: the reply streams in, a confirmation asks, and allowing lets the run finish", async () => {
    const { user } = await renderApp({ route: "/chat/ses_ui", mode: "simple" });
    await user.type(box(), "Ordina i PDF in Downloads{Enter}");
    expect(await within(log()).findByText("Inizio guardando cosa c'è nella cartella Downloads.", {}, { timeout: SLOW })).toBeInTheDocument();
    // While it works the message box says so and offers to stop.
    expect(screen.getByRole("button", { name: "Ferma" })).toBeInTheDocument();
    const card = await screen.findByRole("article", { name: "shell" }, { timeout: SLOW });
    expect(within(card).getByText("Serve la tua conferma")).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Consenti" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Ferma" })).toBeNull(), { timeout: SLOW });
    expect(within(log()).getByText("Consentito")).toBeInTheDocument();
  }, 60_000);

  it("refusing the confirmation ends the run with the agent's own words", async () => {
    const { user } = await renderApp({ route: "/chat/ses_pdf", mode: "simple" });
    await user.type(box(), "Ordina i PDF in Downloads{Enter}");
    const card = await screen.findByRole("article", { name: "shell" }, { timeout: SLOW });
    await user.click(within(card).getByRole("button", { name: "Nega" }));
    expect(await within(log()).findByText(/Ok, non sposto nulla/, {}, { timeout: SLOW })).toBeInTheDocument();
    expect(within(screen.getAllByRole("article", { name: "shell" }).at(-1)!).getByText("Negato")).toBeInTheDocument();
  }, 60_000);
});

describe("chat, Developer interface", () => {
  it("has every section next to the chat, the choice of the agent for a new chat and the prompts of the MCP servers", async () => {
    await renderApp({ route: "/chat/ses_pdf", mode: "developer" });
    const nav = screen.getByRole("navigation", { name: "Sezioni" });
    for (const name of ["Runtime", "Sicurezza", "Attività", "Impostazioni", "Modelli"]) expect(within(nav).getByRole("link", { name })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Agente della nuova sessione" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Prompt MCP" })).toBeInTheDocument();
    expect(location.hash).toBe("#/chat/ses_pdf");
  });

  it("a failed run offers the way to the runtime, not a question to the Assistente", async () => {
    await renderApp({ route: "/chat/ses_bug", mode: "developer" });
    expect(await within(log()).findByText(/memoria video esaurita/)).toBeInTheDocument();
    expect(within(log()).queryByRole("button", { name: /^Chiedi all'assistente/ })).toBeNull();
    expect(within(log()).getByRole("link", { name: /Apri i runtime/ })).toHaveAttribute("href", "#/runtimes");
  });

  it("the confirmation offers «Consenti per la sessione», and the session keeps what it was told", async () => {
    const { user } = await renderApp({ route: "/chat/ses_ui", mode: "developer" });
    await user.type(box(), "Ordina i PDF in Downloads{Enter}");
    const card = await screen.findByRole("article", { name: "shell" }, { timeout: SLOW });
    await user.click(within(card).getByRole("button", { name: "Consenti per la sessione" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Ferma" })).toBeNull(), { timeout: SLOW });
    expect(within(log()).getByText("Consentito")).toBeInTheDocument();
  }, 60_000);
});
