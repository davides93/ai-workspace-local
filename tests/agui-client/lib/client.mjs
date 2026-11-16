// I due client AG-UI reali che la prova usa, e un lettore di run che raccoglie tutto ciò che il client ha visto.
// `@ag-ui/client` 1.0.4 è l'ultima versione; 1.0.2 è quella inclusa in `@copilotkit/runtime` 1.88.0 (il runtime di
// CopilotKit parla con un agente remoto attraverso `HttpAgent`): lo stesso scenario deve funzionare con entrambe.

import { randomUUID } from "node:crypto";

export async function loadClients() {
  const latest = await import("@ag-ui/client");
  const old = await import("@ag-ui/client-1.0.2");
  return [
    { name: "@ag-ui/client 1.0.4", HttpAgent: latest.HttpAgent },
    { name: "@ag-ui/client 1.0.2 (come in @copilotkit/runtime 1.88.0)", HttpAgent: old.HttpAgent },
  ];
}

export const uid = (prefix = "id") => `${prefix}-${randomUUID()}`;

/** Un agente HTTP del client `Client` sull'agente AIW `agentId`, con la cronologia e lo stato che il client già ha. */
export function makeAgent(Client, server, agentId, { threadId = uid("thr"), messages = [], state = {} } = {}) {
  const agent = new Client.HttpAgent({ url: `${server.base}/agui/${agentId}`, threadId });
  agent.messages = messages;
  agent.state = state;
  return agent;
}

export const userMessage = (content, id = uid("msg")) => ({ id, role: "user", content });

/**
 * Fa un run e raccoglie ciò che il client ha visto: gli eventi (dopo la validazione del client), l'esito
 * `RUN_FINISHED`, un eventuale `RUN_ERROR` e un fallimento del client stesso (evento fuori forma o fuori ordine).
 */
export async function runCollect(agent, params = {}) {
  const events = [];
  let outcome;
  let runError;
  let failure;
  try {
    await agent.runAgent(params, {
      onEvent: ({ event }) => {
        events.push(event);
      },
      onRunFinishedEvent: ({ event }) => {
        outcome = event.outcome;
      },
      onRunErrorEvent: ({ event }) => {
        runError = event;
      },
      onRunFailed: ({ error }) => {
        failure = error;
      },
    });
  } catch (e) {
    failure = failure ?? e;
  }
  return { events, types: events.map((e) => e.type), outcome, runError, failure };
}

/** Il testo dei messaggi dell'assistente, nell'ordine. */
export const assistantTexts = (agent) => agent.messages.filter((m) => m.role === "assistant" && m.content).map((m) => m.content);

export const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

/** Aspetta che `cond()` (anche asincrona) dia un valore vero; lo restituisce. */
export async function until(what, cond, { timeout = 15_000, every = 50 } = {}) {
  const deadline = Date.now() + timeout;
  for (;;) {
    const v = await cond();
    if (v) return v;
    if (Date.now() > deadline) throw new Error(`tempo scaduto: ${what}`);
    await sleep(every);
  }
}

/**
 * Una richiesta `POST /agui/{agent}` scritta a mano, per ciò che il client vero non lascia partire (rifiuta da solo un `resume`
 * che non risponde a tutte le interruzioni aperte): restituisce lo stato HTTP e gli eventi dello stream, come arrivano.
 */
export async function rawRun(server, agentId, body) {
  const res = await fetch(`${server.base}/agui/${agentId}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const raw = await res.text();
  const events = raw.split("\n").filter((l) => l.startsWith("data:")).map((l) => JSON.parse(l.slice(5)));
  return { status: res.status, events, raw };
}
