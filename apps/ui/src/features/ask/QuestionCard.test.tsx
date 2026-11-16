import { act, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ApiError } from "../../shared/api/http";
import { MockTransport } from "../../shared/api/mock";
import type { Question } from "../../shared/api/types";
import type { QuestionItem } from "../chat/model/items";
import { renderIn } from "../../test-utils/render";
import { QuestionCard } from "./QuestionCard";

// The card as a person uses it: the keyboard, the focus rules, what is sent and what is shown when the answer is refused.
// (The markup is also checked, without a DOM, in render.test.ts; the whole flow in the browser, in e2e/flows/ask.spec.ts.)
const base: Question = {
  id: "ask_1", run_id: "run_1", kind: "choice", title: "Quale modello?", message: "Ho trovato tre varianti.",
  options: [{ id: "q4", label: "Q4_K_M · 4,7 GB", description: "Consigliata" }, { id: "q8", label: "Q8_0 · 8,5 GB", description: null }, { id: "f16", label: "F16 · 15 GB", description: null }],
  multiple: false, allow_other: false, fields: [], confirm_label: null, deny_label: null,
};
const item = (q: Partial<Question>, extra: Partial<QuestionItem> = {}): QuestionItem => ({ kind: "question", id: "ask_1", q: { ...base, ...q }, ...extra });

function setup(q: Partial<Question> = {}, answer?: (id: string, reply: unknown) => Promise<void>) {
  const api = new MockTransport();
  const send = vi.spyOn(api, "answerQuestion").mockImplementation(async (id, reply) => { await answer?.(id, reply); });
  const view = renderIn(<QuestionCard it={item(q)} />, api);
  return { ...view, send };
}

describe("QuestionCard: a choice", () => {
  it("sends the option that was picked, with Enter from the option or with the button", async () => {
    const { user, send } = setup();
    await user.click(screen.getByRole("radio", { name: /Q8_0/ }));
    await user.keyboard("{Enter}");
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith("ask_1", { action: "answer", selected: ["q8"], other: null, confirmed: null, values: null });
    // Once sent the card is closed to further answers (the row closes when the engine says so): nothing goes twice.
    await waitFor(() => expect(screen.getByRole("button", { name: "Invia" })).toBeDisabled());
    expect(screen.getByRole("radio", { name: /Q8_0/ })).toBeDisabled();
    await user.keyboard("{Enter}");
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("the digits pick the option and move the focus to it, only while the focus is on the options", async () => {
    const { user, send } = setup();
    const first = screen.getByRole("radio", { name: /Q4_K_M/ });
    await waitFor(() => expect(document.activeElement).toBe(first));
    await user.keyboard("3");
    expect(screen.getByRole("radio", { name: /F16/ })).toBeChecked();
    expect(document.activeElement).toBe(screen.getByRole("radio", { name: /F16/ }));
    await user.keyboard("1");
    expect(first).toBeChecked();
    expect(send).not.toHaveBeenCalled();
    // A digit that has no option does nothing.
    await user.keyboard("7");
    expect(first).toBeChecked();
  });

  it("without a choice it asks for one and sends nothing", async () => {
    const { user, send } = setup();
    await user.click(screen.getByRole("button", { name: "Invia" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Scegli un'opzione");
    expect(send).not.toHaveBeenCalled();
  });

  it("«Altro»: the digits do not act while the person types in its field; the text is trimmed and sent", async () => {
    const { user, send } = setup({ allow_other: true });
    const other = screen.getByRole("textbox", { name: "Scrivi la tua risposta" });
    await user.click(other);
    // Focusing the field picks «Altro»; typing a digit writes it.
    expect(screen.getAllByRole("radio")[3]).toBeChecked();
    await user.keyboard("  Q5_K_M 2 ");
    expect(other).toHaveValue("  Q5_K_M 2 ");
    expect(screen.getAllByRole("radio")[0]).not.toBeChecked();
    await user.keyboard("{Enter}");
    expect(send).toHaveBeenCalledWith("ask_1", { action: "answer", selected: null, other: "Q5_K_M 2", confirmed: null, values: null });
  });

  it("«Altro» chosen and left empty is not an answer", async () => {
    const { user, send } = setup({ allow_other: true });
    await user.click(screen.getAllByRole("radio")[3]);
    await user.click(screen.getByRole("button", { name: "Invia" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Scrivi la tua risposta in «Altro».");
    expect(send).not.toHaveBeenCalled();
  });

  it("several answers are checkboxes, sent in the order of the options", async () => {
    const { user, send } = setup({ multiple: true });
    await user.click(screen.getByRole("checkbox", { name: /F16/ }));
    await user.click(screen.getByRole("checkbox", { name: /Q4_K_M/ }));
    await user.click(screen.getByRole("button", { name: "Invia" }));
    expect(send).toHaveBeenCalledWith("ask_1", expect.objectContaining({ action: "answer", selected: expect.arrayContaining(["q4", "f16"]) }));
    expect((send.mock.calls[0][1] as { selected: string[] }).selected).toHaveLength(2);
  });

  it("«Non ora» declines", async () => {
    const { user, send } = setup();
    await user.click(screen.getByRole("button", { name: "Non ora" }));
    expect(send).toHaveBeenCalledWith("ask_1", { action: "decline" });
  });
});

describe("QuestionCard: confirm and form", () => {
  it("a confirmation has the two labels it was given and sends the choice", async () => {
    const { user, send } = setup({ kind: "confirm", options: [], confirm_label: "Scarica", deny_label: "Lascia stare" });
    await user.click(screen.getByRole("button", { name: "Lascia stare" }));
    expect(send).toHaveBeenLastCalledWith("ask_1", expect.objectContaining({ action: "answer", confirmed: false }));
  });

  it("the focus goes to the main button of a confirmation when it appears", async () => {
    setup({ kind: "confirm", options: [], confirm_label: "Scarica", deny_label: "Lascia stare" });
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Scarica" })));
  });

  it("a form: a required field left empty is named in the error, then the typed values go as the right types", async () => {
    const fields = [
      { name: "nome", label: "Nome", type: "string" as const, required: true },
      { name: "eta", label: "Età", type: "integer" as const },
      { name: "ok", label: "Va bene", type: "boolean" as const },
    ];
    const { user, send } = setup({ kind: "form", options: [], fields });
    await user.click(screen.getByRole("button", { name: "Invia" }));
    expect(screen.getByRole("alert").textContent).toMatch(/Nome/);
    expect(send).not.toHaveBeenCalled();
    await user.type(screen.getByLabelText("Nome *"), "Anna");
    await user.type(screen.getByLabelText("Età"), "41");
    await user.click(screen.getByRole("checkbox", { name: "Va bene" }));
    await user.keyboard("{Enter}");
    expect(send).toHaveBeenCalledWith("ask_1", { action: "answer", selected: null, other: null, confirmed: null, values: { nome: "Anna", eta: 41, ok: true } });
  });
});

describe("QuestionCard: when the answer does not go through", () => {
  it("a refused answer keeps the question open with the reason, and the person can try again", async () => {
    let n = 0;
    const { user, send } = setup({}, async () => { if (n++ === 0) throw new ApiError(500, "il motore non risponde"); });
    await user.click(screen.getByRole("radio", { name: /Q4_K_M/ }));
    await user.click(screen.getByRole("button", { name: "Invia" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("il motore non risponde");
    expect(screen.getByRole("button", { name: "Invia" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Invia" }));
    await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("an answer that comes after the question ended (404) says so and cannot be sent again", async () => {
    const { user, send } = setup({}, async () => { throw new ApiError(404, "domanda non trovata"); });
    await user.click(screen.getByRole("radio", { name: /Q4_K_M/ }));
    await user.click(screen.getByRole("button", { name: "Invia" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Questa domanda è già stata chiusa.");
    expect(screen.getByRole("button", { name: "Invia" })).toBeDisabled();
    expect(send).toHaveBeenCalledTimes(1);
  });
});

describe("QuestionCard: the focus", () => {
  it("takes the focus to the first answer when the person was in the message box or nowhere", async () => {
    const box = document.createElement("textarea");
    box.id = "composer";
    document.body.append(box);
    box.focus();
    setup();
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("radio", { name: /Q4_K_M/ })));
    box.remove();
  });

  it("does not take the focus from a field the person is typing in", async () => {
    const other = document.createElement("input");
    document.body.append(other);
    other.focus();
    setup();
    await act(async () => { await new Promise((r) => setTimeout(r, 30)); });
    expect(document.activeElement).toBe(other);
    other.remove();
  });

  it("announces the question for the screen reader a moment after it appears, once", async () => {
    setup();
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("");
    await waitFor(() => expect(status.textContent).toMatch(/Ho trovato tre varianti/));
  });
});

describe("QuestionCard: a question that is closed", () => {
  it("is one line, with no form, no field and no focusable control", () => {
    const { container } = renderIn(<QuestionCard it={item({}, { outcome: "answered", answer: { selected: ["q8"], other: null, confirmed: null, values: null } })} />);
    expect(container.querySelector("form, input, button")).toBeNull();
    expect(container).toHaveTextContent("Hai risposto: Q8_0 · 8,5 GB");
  });
});
