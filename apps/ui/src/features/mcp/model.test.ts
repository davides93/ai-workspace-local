import { describe, expect, it } from "vitest";
import type { McpElicitation } from "../../shared/api/types";
import { initialValues, toContent } from "./model";

const schema: McpElicitation["schema"] = {
  type: "object",
  properties: { nome: { type: "string", title: "Nome" }, eta: { type: "integer" }, ok: { type: "boolean" }, lingua: { type: "string", enum: ["it", "en"] } },
  required: ["nome"],
};

describe("elicitation form", () => {
  it("starts with booleans off and the first value of an enum", () => {
    expect(initialValues(schema)).toEqual({ nome: "", eta: "", ok: false, lingua: "it" });
  });
  it("turns the form into the content of an answer", () => {
    expect(toContent(schema, { nome: " Ada ", eta: "36", ok: true, lingua: "en" })).toEqual({ content: { nome: "Ada", eta: 36, ok: true, lingua: "en" } });
    expect(toContent(schema, { nome: "Ada", eta: "", ok: false, lingua: "it" })).toEqual({ content: { nome: "Ada", ok: false, lingua: "it" } });
  });
  it("says what is wrong", () => {
    expect(toContent(schema, { nome: " ", eta: "", ok: false, lingua: "it" })).toEqual({ error: "Compila «Nome»" });
    expect(toContent(schema, { nome: "a", eta: "3.5", ok: false, lingua: "it" })).toMatchObject({ error: expect.stringContaining("intero") });
    expect(toContent(schema, { nome: "a", eta: "x", ok: false, lingua: "it" })).toMatchObject({ error: expect.stringContaining("numero") });
    expect(toContent(schema, { nome: "a", eta: "", ok: false, lingua: "fr" })).toMatchObject({ error: expect.stringContaining("valori proposti") });
  });
});
