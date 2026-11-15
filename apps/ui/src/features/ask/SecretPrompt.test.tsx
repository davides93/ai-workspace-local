import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ApiError } from "../../shared/api/http";
import { MockTransport } from "../../shared/api/mock";
import type { SecretRequest } from "../../shared/api/types";
import type { SecretItem } from "../chat/model/items";
import { renderIn } from "../../test-utils/render";
import { SecretPrompt } from "./SecretPrompt";

// The invariant of this card: the value lives in the field and in the body of the answer, and nowhere else. These tests look for it
// in the page, in the storage of the browser and in the caches of the app, as the end-to-end test does for the whole app.
const VALUE = "hf_TEST_ONLY_not_a_real_token_0000";
const req: SecretRequest = { id: "sec_1", run_id: "run_1", target: "provider_token:hf", label: "Token di Hugging Face", reason: "Serve per scaricare modelli riservati", destination: "Hugging Face (huggingface.co)" };
const item = (extra: Partial<SecretItem> = {}): SecretItem => ({ kind: "secret", id: "sec_1", req, ...extra });

function setup(answer: (reply: unknown) => Promise<{ stored?: boolean }> = async () => ({ stored: true })) {
  const api = new MockTransport();
  const send = vi.spyOn(api, "answerSecretRequest").mockImplementation(async (_id, reply) => answer(reply));
  const view = renderIn(<SecretPrompt it={item()} />, api);
  const field = () => screen.getByLabelText("Valore di «Token di Hugging Face»") as HTMLInputElement;
  return { ...view, send, field };
}

/** Everything the page and the app keep, as text: where a leaked value would show. */
const everything = (client: { getQueryCache: () => { getAll: () => unknown[] }; getMutationCache: () => { getAll: () => unknown[] } }) =>
  [
    document.documentElement.outerHTML,
    [...document.querySelectorAll("input, textarea, select")].map((e) => (e as HTMLInputElement).value).join("\n"),
    JSON.stringify(Object.fromEntries(Object.entries(localStorage))),
    JSON.stringify(Object.fromEntries(Object.entries(sessionStorage))),
    JSON.stringify(client.getQueryCache().getAll().map((q) => ({ key: (q as { queryKey: unknown }).queryKey, state: (q as { state: unknown }).state }))),
    JSON.stringify(client.getMutationCache().getAll().map((m) => (m as { state: unknown }).state)),
  ].join("\n");

describe("SecretPrompt", () => {
  it("the field is hidden by default and the toggle shows it and hides it again, with a name that does not change", async () => {
    const { user, field } = setup();
    expect(field()).toHaveAttribute("type", "password");
    const toggle = screen.getByRole("button", { name: "Mostra il valore" });
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    await user.type(field(), "abc");
    await user.click(toggle);
    expect(field()).toHaveAttribute("type", "text");
    expect(screen.getByRole("button", { name: "Mostra il valore" })).toHaveAttribute("aria-pressed", "true");
    expect(field()).toHaveValue("abc");
    await user.click(screen.getByRole("button", { name: "Mostra il valore" }));
    expect(field()).toHaveAttribute("type", "password");
  });

  it("«Salva» waits for a value", async () => {
    const { user, field } = setup();
    expect(screen.getByRole("button", { name: "Salva" })).toBeDisabled();
    await user.type(field(), "x");
    expect(screen.getByRole("button", { name: "Salva" })).toBeEnabled();
  });

  it("sends the value in the answer, and afterwards it is nowhere: not in the page, not in the storage, not in the caches", async () => {
    const { user, field, send, client } = setup();
    await user.type(field(), VALUE);
    expect(everything(client)).toContain(VALUE); // the field itself holds it while it is typed: the search can find it
    await user.keyboard("{Enter}");
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    expect(send).toHaveBeenCalledWith("sec_1", { action: "provide", value: VALUE });
    expect(field()).toHaveValue("");
    expect(everything(client)).not.toContain(VALUE);
    expect(screen.getByRole("button", { name: "Salva" })).toBeDisabled();
    // Nothing goes twice while the row waits to be closed by the engine.
    await user.keyboard("{Enter}");
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("a value that is refused is not kept to be sent again: the field is empty and has the focus, and the message does not repeat the value", async () => {
    const { user, field, send, client } = setup(async () => { throw new ApiError(401, `Hugging Face ha rifiutato ${VALUE}: non sembra valido.`); });
    await user.type(field(), VALUE);
    await user.click(screen.getByRole("button", { name: "Salva" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Il valore non è stato accettato");
    expect(alert.textContent).not.toContain(VALUE);
    expect(alert.textContent).toContain("•••");
    expect(field()).toHaveValue("");
    await waitFor(() => expect(document.activeElement).toBe(field()));
    expect(everything(client)).not.toContain(VALUE);
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("a failure that is not about the value (the engine is down) keeps the card for another try and does not repeat the value either", async () => {
    const { user, field, client } = setup(async () => { throw new ApiError(500, `errore interno con ${VALUE}`); });
    await user.type(field(), VALUE);
    await user.click(screen.getByRole("button", { name: "Salva" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).not.toContain(VALUE);
    expect(field()).toBeEnabled();
    expect(everything(client)).not.toContain(VALUE);
  });

  it("«Non ora» declines and drops what was typed", async () => {
    const { user, field, send, client } = setup();
    await user.type(field(), VALUE);
    await user.click(screen.getByRole("button", { name: "Non ora" }));
    expect(send).toHaveBeenCalledWith("sec_1", { action: "decline" });
    expect(everything(client)).not.toContain(VALUE);
  });

  it("a device that may not provide secrets (403) says so and has no field at all", async () => {
    const { user, field, send } = setup(async () => { throw new ApiError(403, "questo dispositivo non può fornire segreti"); });
    await user.type(field(), "qualcosa");
    await user.click(screen.getByRole("button", { name: "Salva" }));
    await waitFor(() => expect(screen.getByText("Questo dispositivo non può inserire valori riservati.")).toBeInTheDocument());
    expect(screen.queryByLabelText("Valore di «Token di Hugging Face»")).toBeNull();
    expect(screen.queryByRole("button", { name: "Salva" })).toBeNull();
    expect(send).toHaveBeenCalledTimes(1);
    expect(document.body.innerHTML).not.toContain("qualcosa");
  });

  it("a request that was closed in the meantime (404) says so and cannot be answered again", async () => {
    const { user, field, send } = setup(async () => { throw new ApiError(404, "richiesta non trovata"); });
    await user.type(field(), VALUE);
    await user.click(screen.getByRole("button", { name: "Salva" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Questa richiesta è già stata chiusa.");
    expect(field()).toBeDisabled();
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("when the card goes away the field is emptied too", async () => {
    const { user, field, unmount } = setup();
    const el = field();
    await user.type(el, VALUE);
    unmount();
    expect(el.value).toBe("");
  });

  it("says what it is for and where the value goes, and that on this computer it is encrypted with a key in the keychain", async () => {
    setup();
    expect(screen.getByText("Serve per scaricare modelli riservati")).toBeInTheDocument();
    expect(screen.getByText("Sarà salvato in: Hugging Face (huggingface.co)")).toBeInTheDocument();
    expect(await screen.findByText(/Su questo computer è cifrato, con una chiave custodita nel portachiavi del sistema/)).toBeInTheDocument();
    expect(screen.queryByText(/salvato senza cifratura/)).toBeNull();
  });

  it("says that it is kept without encryption when the system keychain is not available", async () => {
    const api = new MockTransport();
    vi.spyOn(api, "secretsAtRest").mockResolvedValue({ state: "unavailable", backend: "", encrypted: 0, plain: 2, unreadable: 0 });
    renderIn(<SecretPrompt it={item()} />, api);
    expect(await screen.findByText(/Su questo computer è salvato senza cifratura/)).toBeInTheDocument();
    expect(screen.queryByText(/è cifrato, con una chiave/)).toBeNull();
  });

  it("a closed request is one line that says only that it was saved, with no field and no value", () => {
    const { container } = renderIn(<SecretPrompt it={item({ outcome: "stored" })} />);
    expect(container).toHaveTextContent("Salvato: Token di Hugging Face");
    expect(container.querySelector("input, form, button")).toBeNull();
  });
});
