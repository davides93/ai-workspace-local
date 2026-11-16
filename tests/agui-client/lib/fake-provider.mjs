// Un provider OpenAI-compatibile finto (`/v1/models`, `/v1/chat/completions` in streaming) con risposte
// scritte a mano: ogni richiesta di chat consuma il turno successivo della sceneggiatura. Serve a far
// girare il vero `aiw serve` senza un modello. Registra le richieste, così i test possono controllare che
// cosa ha visto il «modello» (contesto, messaggi, assenza di segreti).

import http from "node:http";

/** Un turno di testo: l'assistente risponde con `text` (in più pezzi) e, se c'è, con `reasoning`. */
export const text = (t, reasoning) => ({ text: t, reasoning });
/** Un turno con una chiamata di strumento (gli argomenti sono un oggetto, che qui si serializza). */
export const call = (name, args, id = `call_${name}`) => ({ calls: [{ name, args, id }] });
/** Un turno con più chiamate di strumento insieme: `[[nome, argomenti, id?], …]`. */
export const calls = (list) => ({ calls: list.map(([name, args, id], i) => ({ name, args, id: id ?? `call_${name}_${i}` })) });

function chunk(delta, finish = null) {
  return { id: "chatcmpl-fake", object: "chat.completion.chunk", created: 0, model: "fake-model", choices: [{ index: 0, delta, finish_reason: finish }] };
}

/** Avvia il provider su una porta libera di 127.0.0.1. */
export async function startFakeProvider() {
  let script = [];
  const requests = [];
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    if (req.method === "GET" && url.pathname === "/v1/models") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ object: "list", data: [{ id: "fake-model", object: "model" }] }));
      return;
    }
    if (req.method === "POST" && url.pathname === "/v1/chat/completions") {
      let raw = "";
      req.on("data", (c) => (raw += c));
      req.on("end", () => {
        const body = JSON.parse(raw || "{}");
        requests.push(body);
        const turn = script.shift() ?? text("(il provider finto non ha altri turni)");
        res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
        const send = (o) => res.write(`data: ${JSON.stringify(o)}\n\n`);
        send(chunk({ role: "assistant", content: "" }));
        if (turn.reasoning) send(chunk({ reasoning_content: turn.reasoning }));
        if (turn.calls) {
          turn.calls.forEach((c, index) => {
            const args = JSON.stringify(c.args);
            const half = Math.ceil(args.length / 2);
            send(chunk({ tool_calls: [{ index, id: c.id, type: "function", function: { name: c.name, arguments: args.slice(0, half) } }] }));
            send(chunk({ tool_calls: [{ index, function: { arguments: args.slice(half) } }] }));
          });
          send(chunk({}, "tool_calls"));
        } else {
          const t = turn.text ?? "";
          const mid = Math.ceil(t.length / 2);
          for (const part of [t.slice(0, mid), t.slice(mid)]) if (part) send(chunk({ content: part }));
          send(chunk({}, "stop"));
        }
        res.write(`data: ${JSON.stringify({ id: "chatcmpl-fake", object: "chat.completion.chunk", choices: [], usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } })}\n\n`);
        res.write("data: [DONE]\n\n");
        res.end();
      });
      return;
    }
    res.writeHead(404, { "content-type": "application/json" });
    res.end(JSON.stringify({ error: { message: `non gestito: ${req.method} ${url.pathname}` } }));
  });
  await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
  const port = server.address().port;
  return {
    /** Base URL che termina con `/v1`, da dare al runtime `remote-openai`. */
    base: `http://127.0.0.1:${port}/v1`,
    /** Sostituisce la sceneggiatura (un turno per richiesta di chat). */
    set(turns) {
      script = [...turns];
    },
    /** Turni non ancora consumati. */
    remaining: () => script.length,
    /** Corpi delle richieste di chat ricevute finora. */
    requests,
    close: () => new Promise((ok) => server.close(ok)),
  };
}
