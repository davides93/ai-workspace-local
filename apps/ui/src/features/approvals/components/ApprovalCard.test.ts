import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ApprovalRequest } from "../../../shared/api/types";
import { ApprovalCard } from "./ApprovalCard";

const req = (tool: string): ApprovalRequest => ({ id: "a1", run_id: "run_1", tool, args: { model: "qwen" }, reason: "Scarica un modello.", risk: "medium" });
const html = (tool: string, extra: { allowRemember?: boolean; afterExternal?: boolean; resolved?: boolean } = {}) =>
  renderToStaticMarkup(createElement(ApprovalCard, { request: req(tool), onRespond: () => {}, ...extra }));

describe("ApprovalCard", () => {
  it("offers «Consenti per la sessione» for an ordinary tool when the caller allows it", () => {
    expect(html("shell", { allowRemember: true })).toContain("Consenti per la sessione");
    expect(html("shell")).not.toContain("Consenti per la sessione");
  });

  it("never offers it for a management tool (aiw_*), whatever the caller asks", () => {
    for (const tool of ["aiw_model_download", "aiw_setting_set_protected", "aiw_secret_clear"]) {
      const out = html(tool, { allowRemember: true });
      expect(out, tool).not.toContain("Consenti per la sessione");
      // The two real choices are still there.
      expect(out).toContain("Consenti");
      expect(out).toContain("Nega");
    }
  });

  it("says that the request came after reading outside content, only while it waits", () => {
    expect(html("aiw_model_download", { afterExternal: true })).toContain("Richiesta arrivata dopo la lettura di contenuti esterni");
    expect(html("aiw_model_download")).not.toContain("contenuti esterni");
    expect(html("aiw_model_download", { afterExternal: true, resolved: true })).not.toContain("contenuti esterni");
  });
});
