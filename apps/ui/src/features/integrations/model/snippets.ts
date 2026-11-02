/** Copyable examples for every endpoint AI Workspace exposes to other programs. Pure, so they are testable. */

export interface Endpoint {
  id: "openai" | "anthropic" | "mcp" | "a2a" | "webhook";
  title: string;
  description: string;
  /** Main URL to copy. */
  url: string;
  /** Extra URLs (agent card, model list…). */
  extra?: { label: string; url: string }[];
  snippets: { title: string; code: string }[];
}

const strip = (s: string) => s.replace(/\/+$/, "");

export function buildEndpoints(baseUrl: string): Endpoint[] {
  const base = strip(baseUrl);
  return [
    {
      id: "openai",
      title: "API compatibile OpenAI",
      description: "Usa i modelli locali da qualsiasi client o SDK OpenAI: cambia solo la base URL.",
      url: `${base}/v1`,
      extra: [{ label: "Elenco dei modelli", url: `${base}/v1/models` }],
      snippets: [
        {
          title: "curl",
          code: `curl ${base}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -d '{"model": "balanced", "messages": [{"role": "user", "content": "Ciao!"}]}'`,
        },
        {
          title: "SDK Python",
          code: `from openai import OpenAI

client = OpenAI(base_url="${base}/v1", api_key="locale")
r = client.chat.completions.create(
    model="balanced",
    messages=[{"role": "user", "content": "Ciao!"}],
)
print(r.choices[0].message.content)`,
        },
      ],
    },
    {
      id: "anthropic",
      title: "API compatibile Anthropic",
      description: "Messages API per gli SDK e gli strumenti che parlano il protocollo Anthropic (`/v1/messages`).",
      url: `${base}/v1/messages`,
      extra: [{ label: "Conteggio dei token", url: `${base}/v1/messages/count_tokens` }],
      snippets: [
        {
          title: "curl",
          code: `curl ${base}/v1/messages \\
  -H "Content-Type: application/json" \\
  -H "anthropic-version: 2023-06-01" \\
  -d '{"model": "balanced", "max_tokens": 256, "messages": [{"role": "user", "content": "Ciao!"}]}'`,
        },
        {
          title: "SDK Python",
          code: `import anthropic

client = anthropic.Anthropic(base_url="${base}", api_key="locale")
m = client.messages.create(
    model="balanced",
    max_tokens=256,
    messages=[{"role": "user", "content": "Ciao!"}],
)
print(m.content[0].text)`,
        },
      ],
    },
    {
      id: "mcp",
      title: "Server MCP",
      description: "Espone i tool e gli agenti di AI Workspace a client MCP (editor, altri agenti) via streamable HTTP.",
      url: `${base}/mcp`,
      snippets: [
        {
          title: "Configurazione del client MCP",
          code: `{
  "mcpServers": {
    "ai-workspace": { "url": "${base}/mcp" }
  }
}`,
        },
        {
          title: "curl (elenco dei tool)",
          code: `curl ${base}/mcp \\
  -H "Content-Type: application/json" \\
  -H "Accept: application/json, text/event-stream" \\
  -d '{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}'`,
        },
      ],
    },
    {
      id: "a2a",
      title: "A2A (agent to agent)",
      description: "Altri agenti scoprono AI Workspace dalla agent card e gli affidano task sull'endpoint A2A.",
      url: `${base}/a2a`,
      extra: [{ label: "Agent card", url: `${base}/.well-known/agent-card.json` }],
      snippets: [
        { title: "curl (agent card)", code: `curl ${base}/.well-known/agent-card.json` },
        {
          title: "curl (invia un messaggio)",
          code: `curl ${base}/a2a \\
  -H "Content-Type: application/json" \\
  -d '{"jsonrpc": "2.0", "id": 1, "method": "message/send", "params": {"message": {"role": "user", "parts": [{"text": "Riordina i PDF in Downloads"}], "messageId": "m1"}}}'`,
        },
      ],
    },
    {
      id: "webhook",
      title: "Webhook dei workflow",
      description: "Servizi esterni avviano un workflow con una POST: il JSON ricevuto diventa gli input. Sostituisci l'id con quello del workflow.",
      url: `${base}/hooks`,
      snippets: [
        {
          title: "curl",
          code: `curl -X POST ${base}/hooks/ordine-ricevuto \\
  -H "Content-Type: application/json" \\
  -H "X-AIW-Secret: <segreto-del-workflow>" \\
  -d '{"order_id": "A-1042", "customer": "Rossi Srl"}'`,
        },
      ],
    },
  ];
}
