import { t } from "../../../shared/i18n";
/** Copyable examples for every endpoint AI Workspace exposes to other programs. Pure, so they are testable. */

export type SnippetLang = "curl" | "python" | "javascript" | "config";

export interface Snippet {
  lang: SnippetLang;
  /** Tab label: the language and, where it matters, the library. */
  label: string;
  code: string;
}

export interface Endpoint {
  id: "openai" | "anthropic" | "mcp" | "a2a" | "webhook";
  title: string;
  description: string;
  /** Main URL to copy. */
  url: string;
  /** Extra URLs (agent card, model list…). */
  extra?: { label: string; url: string }[];
  /** Needs the API token when the call does not come from this computer (webhooks carry their own secret). */
  guarded: boolean;
  snippets: Snippet[];
}

export interface BuildOptions {
  /** Add the pairing token to the examples (clients on another device of the LAN). */
  token?: boolean;
}

/** What the user replaces with the token obtained by pairing. */
export const TOKEN_PLACEHOLDER = "<token-di-pairing>";

const strip = (s: string) => s.replace(/\/+$/, "");
const q = (s: string) => JSON.stringify(s);

/** `headers: { … }` of a fetch call, one entry per line. */
function jsHeaders(entries: [string, string][]): string {
  return `{\n${entries.map(([k, v]) => `    ${/^[A-Za-z_]\w*$/.test(k) ? k : q(k)}: ${q(v)},`).join("\n")}\n  }`;
}

export function buildEndpoints(baseUrl: string, opts: BuildOptions = {}): Endpoint[] {
  const base = strip(baseUrl);
  const tok = !!opts.token;
  // The OpenAI gateway takes `Authorization: Bearer`, the Anthropic one `x-api-key` (what each SDK sends).
  const bearer = `Bearer ${TOKEN_PLACEHOLDER}`;
  const curlAuth = tok ? `  -H "Authorization: ${bearer}" \\\n` : "";
  const apiKey = tok ? TOKEN_PLACEHOLDER : "locale";
  const jsonHeader: [string, string][] = [["Content-Type", "application/json"]];
  const withBearer: [string, string][] = tok ? [...jsonHeader, ["Authorization", bearer]] : jsonHeader;

  return [
    {
      id: "openai",
      title: t("integrations.api_compatibile_openai"),
      description: t("integrations.usa_i_modelli_locali_da"),
      url: `${base}/v1`,
      extra: [{ label: t("integrations.elenco_dei_modelli"), url: `${base}/v1/models` }],
      guarded: true,
      snippets: [
        {
          lang: "curl", label: "curl",
          code: `curl ${base}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
${curlAuth}  -d '{"model": "balanced", "messages": [{"role": "user", "content": "Ciao!"}]}'`,
        },
        {
          lang: "python", label: t("integrations.python_sdk_openai"),
          code: `from openai import OpenAI

client = OpenAI(base_url="${base}/v1", api_key="${apiKey}")
r = client.chat.completions.create(
    model="balanced",
    messages=[{"role": "user", "content": "Ciao!"}],
)
print(r.choices[0].message.content)`,
        },
        {
          lang: "javascript", label: t("integrations.javascript_fetch"),
          code: `const res = await fetch("${base}/v1/chat/completions", {
  method: "POST",
  headers: ${jsHeaders(withBearer)},
  body: JSON.stringify({
    model: "balanced",
    messages: [{ role: "user", content: "Ciao!" }],
  }),
});
const data = await res.json();
console.log(data.choices[0].message.content);`,
        },
      ],
    },
    {
      id: "anthropic",
      title: t("integrations.api_compatibile_anthropic"),
      description: t("integrations.messages_api_per_gli_sdk"),
      url: `${base}/v1/messages`,
      extra: [{ label: t("integrations.conteggio_dei_token"), url: `${base}/v1/messages/count_tokens` }],
      guarded: true,
      snippets: [
        {
          lang: "curl", label: "curl",
          code: `curl ${base}/v1/messages \\
  -H "Content-Type: application/json" \\
  -H "anthropic-version: 2023-06-01" \\
${tok ? `  -H "x-api-key: ${TOKEN_PLACEHOLDER}" \\\n` : ""}  -d '{"model": "balanced", "max_tokens": 256, "messages": [{"role": "user", "content": "Ciao!"}]}'`,
        },
        {
          lang: "python", label: t("integrations.python_sdk_anthropic"),
          code: `import anthropic

client = anthropic.Anthropic(base_url="${base}", api_key="${apiKey}")
m = client.messages.create(
    model="balanced",
    max_tokens=256,
    messages=[{"role": "user", "content": "Ciao!"}],
)
print(m.content[0].text)`,
        },
        {
          lang: "javascript", label: t("integrations.javascript_fetch"),
          code: `const res = await fetch("${base}/v1/messages", {
  method: "POST",
  headers: ${jsHeaders([...jsonHeader, ["anthropic-version", "2023-06-01"], ...(tok ? ([["x-api-key", TOKEN_PLACEHOLDER]] as [string, string][]) : [])])},
  body: JSON.stringify({
    model: "balanced",
    max_tokens: 256,
    messages: [{ role: "user", content: "Ciao!" }],
  }),
});
const data = await res.json();
console.log(data.content[0].text);`,
        },
      ],
    },
    {
      id: "mcp",
      title: t("integrations.server_mcp"),
      description: t("integrations.espone_i_tool_e_gli"),
      url: `${base}/mcp`,
      guarded: true,
      snippets: [
        {
          lang: "config", label: t("integrations.configurazione_del_client"),
          code: `{
  "mcpServers": {
    "ai-workspace": {
      "url": "${base}/mcp"${tok ? `,\n      "headers": { "Authorization": "${bearer}" }` : ""}
    }
  }
}`,
        },
        {
          lang: "curl", label: "curl",
          code: `curl ${base}/mcp \\
  -H "Content-Type: application/json" \\
  -H "Accept: application/json, text/event-stream" \\
${curlAuth}  -d '{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}'`,
        },
        {
          lang: "javascript", label: t("integrations.javascript_fetch"),
          code: `const res = await fetch("${base}/mcp", {
  method: "POST",
  headers: ${jsHeaders([...jsonHeader, ["Accept", "application/json, text/event-stream"], ...(tok ? ([["Authorization", bearer]] as [string, string][]) : [])])},
  body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/list" }),
});
console.log(await res.text());`,
        },
      ],
    },
    {
      id: "a2a",
      title: t("integrations.a2a_agent_to_agent"),
      description: t("integrations.altri_agenti_scoprono_ai_workspace"),
      url: `${base}/a2a`,
      extra: [{ label: t("integrations.agent_card"), url: `${base}/.well-known/agent-card.json` }],
      guarded: true,
      snippets: [
        {
          lang: "curl", label: "curl",
          code: `# La agent card è pubblica
curl ${base}/.well-known/agent-card.json

# Invia un messaggio
curl ${base}/a2a \\
  -H "Content-Type: application/json" \\
${curlAuth}  -d '{"jsonrpc": "2.0", "id": 1, "method": "message/send", "params": {"message": {"role": "user", "parts": [{"text": "Riordina i PDF in Downloads"}], "messageId": "m1"}}}'`,
        },
        {
          lang: "python", label: t("integrations.python_requests"),
          code: `import requests

card = requests.get("${base}/.well-known/agent-card.json").json()
r = requests.post(
    "${base}/a2a",
    headers=${tok ? `{"Authorization": "${bearer}"}` : "{}"},
    json={
        "jsonrpc": "2.0", "id": 1, "method": "message/send",
        "params": {"message": {"role": "user", "parts": [{"text": "Riordina i PDF in Downloads"}], "messageId": "m1"}},
    },
)
print(card["name"], r.json())`,
        },
        {
          lang: "javascript", label: t("integrations.javascript_fetch"),
          code: `const card = await (await fetch("${base}/.well-known/agent-card.json")).json();
const res = await fetch("${base}/a2a", {
  method: "POST",
  headers: ${jsHeaders(withBearer)},
  body: JSON.stringify({
    jsonrpc: "2.0", id: 1, method: "message/send",
    params: { message: { role: "user", parts: [{ text: "Riordina i PDF in Downloads" }], messageId: "m1" } },
  }),
});
console.log(card.name, await res.json());`,
        },
      ],
    },
    {
      id: "webhook",
      title: t("integrations.webhook_dei_workflow"),
      description: t("integrations.servizi_esterni_avviano_un_workflow"),
      url: `${base}/hooks`,
      guarded: false,
      snippets: [
        {
          lang: "curl", label: "curl",
          code: `curl -X POST ${base}/hooks/ordine-ricevuto \\
  -H "Content-Type: application/json" \\
  -H "X-AIW-Secret: <segreto-del-workflow>" \\
  -d '{"order_id": "A-1042", "customer": "Rossi Srl"}'`,
        },
        {
          lang: "python", label: t("integrations.python_requests"),
          code: `import requests

r = requests.post(
    "${base}/hooks/ordine-ricevuto",
    headers={"X-AIW-Secret": "<segreto-del-workflow>"},
    json={"order_id": "A-1042", "customer": "Rossi Srl"},
)
print(r.status_code, r.text)`,
        },
        {
          lang: "javascript", label: t("integrations.javascript_fetch"),
          code: `const res = await fetch("${base}/hooks/ordine-ricevuto", {
  method: "POST",
  headers: ${jsHeaders([...jsonHeader, ["X-AIW-Secret", "<segreto-del-workflow>"]])},
  body: JSON.stringify({ order_id: "A-1042", customer: "Rossi Srl" }),
});
console.log(res.status, await res.text());`,
        },
      ],
    },
  ];
}

/** Base URL of the examples for a client on another device: the LAN address of this one, or a placeholder. */
export function lanBaseUrl(address: string | null | undefined, port = 7777): string {
  return address ? address.replace(/\/+$/, "") : `http://<ip-del-pc>:${port}`;
}
