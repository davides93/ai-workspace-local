import type { McpElicitation } from "../../shared/api/types";

import { t } from "../../shared/i18n";
export type FormValues = Record<string, string | boolean>;
type Props = McpElicitation["schema"]["properties"];

/** The starting values of the form: booleans off, the first value of an enum, everything else empty. */
export function initialValues(schema: McpElicitation["schema"]): FormValues {
  const out: FormValues = {};
  for (const [name, p] of Object.entries(schema.properties)) {
    out[name] = p.type === "boolean" ? false : p.enum?.[0] ?? "";
  }
  return out;
}

/**
 * The form as the content of an `accept` answer, or the first problem (in Italian). Empty optional fields are
 * left out; numbers are checked here and again by the engine against the schema.
 */
export function toContent(schema: McpElicitation["schema"], values: FormValues): { content: Record<string, string | number | boolean> } | { error: string } {
  const props: Props = schema.properties;
  const content: Record<string, string | number | boolean> = {};
  for (const [name, p] of Object.entries(props)) {
    const label = p.title || name;
    const v = values[name];
    const required = schema.required?.includes(name) ?? false;
    if (p.type === "boolean") {
      content[name] = v === true;
      continue;
    }
    const text = typeof v === "string" ? v.trim() : "";
    if (text === "") {
      if (required) return { error: t("mcp.compila_2", { label }) };
      continue;
    }
    if (p.type === "number" || p.type === "integer") {
      const n = Number(text);
      if (!Number.isFinite(n) || (p.type === "integer" && !Number.isInteger(n))) return { error: t(p.type === "integer" ? "mcp.deve_essere_un_numero_intero" : "mcp.deve_essere_un_numero", { label }) };
      content[name] = n;
    } else {
      if (p.enum && !p.enum.includes(text)) return { error: t("mcp.scegli_uno_dei_valori_proposti", { label }) };
      content[name] = text;
    }
  }
  return { content };
}
