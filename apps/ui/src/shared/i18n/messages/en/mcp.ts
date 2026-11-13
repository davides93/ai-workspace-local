// English messages of "mcp". One entry per line; must have exactly the keys of the Italian file.
import type { mcpIt } from "../it/mcp";

export const mcpEn: Record<keyof typeof mcpIt, string> = {
  "mcp.il_server_ha_una_domanda": "The server {server} has a question",
  "mcp.rifiuta": "Refuse",
  "mcp.annulla": "Cancel",
  "mcp.invia": "Send",
  "mcp.la_domanda_viene_dal_server": "The question comes from the MCP server, not from AI Workspace. Do not enter passwords, keys or other secrets.",
  "mcp.prompt_mcp": "MCP prompt",
  "mcp.carico": "Loading…",
  "mcp.inserisci_nel_messaggio": "Insert into the message",
  "mcp.prompt": "Prompt",
  "mcp.il_testo_viene_inserito_nel": "The text is inserted in the message field: you can edit it before sending.",
  "mcp.risposta_non_inviata": "Answer not sent",
  "mcp.compila": "Fill in \"{name}\"",
  "mcp.il_server_non_ha_dato": "The server didn't return the prompt",
  "mcp.compila_2": "Fill in \"{label}\"",
  "mcp.deve_essere_un_numero": "\"{label}\" must be a number",
  "mcp.deve_essere_un_numero_intero": "\"{label}\" must be a whole number",
  "mcp.scegli_uno_dei_valori_proposti": "\"{label}\": pick one of the suggested values",
};
