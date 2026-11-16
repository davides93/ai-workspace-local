import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ApprovalRequest } from "../../../shared/api/types";
import { renderIn } from "../../../test-utils/render";
import { ApprovalCard, type ApprovalCardProps } from "./ApprovalCard";

// The card as a person uses it (the markup without a DOM is in ApprovalCard.test.ts; the flow in the browser in e2e/flows/approvals.spec.ts).
const request = (over: Partial<ApprovalRequest> = {}): ApprovalRequest => ({
  id: "ap_1", run_id: "run_1", tool: "shell", args: { command: "mkdir 2024 2025", cwd: "~/Downloads" }, reason: "Il comando sposta file nella cartella Downloads.", risk: "medium", ...over,
});
const show = (props: Partial<ApprovalCardProps> = {}) => {
  const onRespond = vi.fn();
  const view = renderIn(<ApprovalCard request={request()} onRespond={onRespond} allowRemember {...props} />);
  return { ...view, onRespond };
};

describe("ApprovalCard", () => {
  it("is an article named by its tool, with the risk, the reason and the three decisions", () => {
    show();
    const card = screen.getByRole("article", { name: "shell" });
    expect(within(card).getByText("rischio medio")).toBeInTheDocument();
    expect(within(card).getByText("Il comando sposta file nella cartella Downloads.")).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Consenti" })).toBeEnabled();
    expect(within(card).getByRole("button", { name: "Consenti per la sessione" })).toBeEnabled();
    expect(within(card).getByRole("button", { name: "Nega" })).toBeEnabled();
  });

  it("each button reports its decision, and only «Consenti per la sessione» asks to remember", async () => {
    const { user, onRespond } = show();
    await user.click(screen.getByRole("button", { name: "Consenti" }));
    await user.click(screen.getByRole("button", { name: "Consenti per la sessione" }));
    await user.click(screen.getByRole("button", { name: "Nega" }));
    expect(onRespond.mock.calls).toEqual([["allow"], ["allow", true], ["deny"]]);
  });

  it("while the answer is on its way every button is off", () => {
    show({ pending: true });
    for (const name of ["Consenti", "Consenti per la sessione", "Nega"]) expect(screen.getByRole("button", { name })).toBeDisabled();
  });

  it("the simple interface does not offer «per la sessione»: the caller does not ask for it", () => {
    show({ allowRemember: false });
    expect(screen.queryByRole("button", { name: "Consenti per la sessione" })).toBeNull();
    expect(screen.getByRole("button", { name: "Consenti" })).toBeInTheDocument();
  });

  it("the arguments are one line, and the JSON opens on demand with its colours", async () => {
    const { user } = show();
    const details = document.querySelector<HTMLElement>(".apv-summary")!.closest("details")!;
    expect(details.querySelector(".apv-summary")).toHaveTextContent("mkdir 2024 2025");
    expect(details).not.toHaveAttribute("open");
    await user.click(within(details).getByText("Argomenti", { selector: ".apv-more" }));
    expect(details).toHaveAttribute("open");
    await waitFor(() => expect(details.querySelector(".tok-attr")).not.toBeNull());
    expect(details.querySelector("pre code")?.textContent).toBe(JSON.stringify({ command: "mkdir 2024 2025", cwd: "~/Downloads" }, null, 2));
  });

  it("a decided card shows the decision and no buttons", () => {
    const { rerender } = show({ resolved: true });
    expect(screen.getByText("Consentito")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Consenti" })).toBeNull();
    rerender(<ApprovalCard request={request()} resolved={false} onRespond={vi.fn()} />);
    expect(screen.getByText("Negato")).toBeInTheDocument();
  });

  it("the shortcut hints (A, N) show only when asked, and are not read out as part of the name", () => {
    show({ showShortcuts: true });
    const allow = screen.getByRole("button", { name: "Consenti" });
    expect(allow).toHaveAttribute("aria-keyshortcuts", "A");
    expect(within(allow).getByText("A")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByRole("button", { name: "Nega" })).toHaveAttribute("aria-keyshortcuts", "N");
  });

  it("a selectable card reports being selected when it is clicked or focused", async () => {
    const onSelect = vi.fn();
    const { user } = show({ onSelect, selected: true });
    expect(screen.getByRole("article", { name: "shell" })).toHaveAttribute("aria-current", "true");
    await user.click(screen.getByRole("button", { name: "Nega" }));
    expect(onSelect).toHaveBeenCalled();
  });

  it("after reading outside content a waiting card says so (and a management tool never offers «per la sessione»)", () => {
    show({ afterExternal: true, request: request({ tool: "aiw_model_download" }) });
    expect(screen.getByText("Richiesta arrivata dopo la lettura di contenuti esterni.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Consenti per la sessione" })).toBeNull();
  });
});
