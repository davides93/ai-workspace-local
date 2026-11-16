// Il runtime di CopilotKit (`@copilotkit/runtime` 1.88.0, lato server) che registra un agente AIW con `HttpAgent` e lo fa girare
// attraverso il suo gestore HTTP (`POST /agent/{agente}/run`), contro il vero `aiw serve` con un provider OpenAI finto. Nessun
// account CopilotKit e nessuna rete: la telemetria del runtime è spenta prima di caricarlo.
//
// Che cosa NON prova: i componenti React di CopilotKit (la parte nel browser), OpenBot e OpenDots (richiedono CopilotKit
// Intelligence), un modello vero.

process.env.COPILOTKIT_TELEMETRY_DISABLED = "true";
process.env.DO_NOT_TRACK = "1";

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { after, before, describe, it } from "node:test";
import { call, startFakeProvider, text } from "../../agui-client/lib/fake-provider.mjs";
import { startServer } from "../../agui-client/lib/server.mjs";

const { HttpAgent } = await import("@ag-ui/client");
const { CopilotRuntime, createCopilotRuntimeHandler, InMemoryAgentRunner } = await import("@copilotkit/runtime/v2");

let prov;
let srv;
let handler;
before(async () => {
  prov = await startFakeProvider();
  srv = await startServer({ providerBase: prov.base });
  const agents = {
    planner: new HttpAgent({ url: `${srv.base}/agui/planner` }),
    operator: new HttpAgent({ url: `${srv.base}/agui/operator` }),
  };
  const runtime = new CopilotRuntime({ agents, runner: new InMemoryAgentRunner() });
  handler = createCopilotRuntimeHandler({ runtime, basePath: "/api/copilotkit", activateChannels: false });
});
after(async () => {
  await srv?.stop();
  await prov?.close();
});

let n = 0;
const uid = (p) => `${p}-${process.pid}-${++n}`;

/** Una `POST /agent/{agente}/run` al gestore del runtime, come la farebbe il frontend di CopilotKit; restituisce gli eventi. */
async function run(agent, body) {
  const res = await handler(
    new Request(`http://localhost/api/copilotkit/agent/${agent}/run`, {
      method: "POST",
      headers: { "content-type": "application/json", accept: "text/event-stream" },
      body: JSON.stringify({ state: {}, tools: [], context: [], forwardedProps: {}, ...body }),
    }),
  );
  assert.equal(res.status, 200, await res.clone().text());
  const events = (await res.text()).split("\n").filter((l) => l.startsWith("data:")).map((l) => JSON.parse(l.slice(5)));
  return { events, types: events.map((e) => e.type) };
}

describe("il runtime di CopilotKit 1.88.0 con un agente AIW", () => {
  it("un messaggio con ragionamento passa dal runtime e arriva intero", async () => {
    prov.set([text("Ciao da CopilotKit", "Penso un attimo.")]);
    const { events, types } = await run("planner", { threadId: uid("thr"), runId: uid("run"), messages: [{ id: uid("m"), role: "user", content: "Saluta" }] });
    assert.equal(types[0], "RUN_STARTED");
    assert.equal(types.at(-1), "RUN_FINISHED");
    assert.ok(types.includes("REASONING_MESSAGE_START") && types.includes("TEXT_MESSAGE_CONTENT"), `${types}`);
    assert.equal(events.filter((e) => e.type === "TEXT_MESSAGE_CONTENT").map((e) => e.delta).join(""), "Ciao da CopilotKit");
    assert.ok(events.find((e) => e.type === "STATE_SNAPSHOT").snapshot.aiw, "lo stato del motore sta sotto `aiw`");
    assert.equal(types.includes("RUN_ERROR"), false);
  });

  it("una conferma è un'interruzione che il runtime lascia passare, e il resume arriva ad AIW", async () => {
    const file = `ck-${uid("f")}.md`;
    prov.set([call("write_file", { path: file, content: "ok" }), text("Scritto")]);
    const threadId = uid("thr");
    const messages = [{ id: uid("m"), role: "user", content: `Scrivi ${file}` }];
    const forwardedProps = { aiw: { interactive: ["interrupts"] } };
    const first = await run("operator", { threadId, runId: uid("run"), messages, forwardedProps });
    const finished = first.events.at(-1);
    assert.equal(finished.type, "RUN_FINISHED");
    assert.equal(finished.outcome?.type, "interrupt", `${first.types}`);
    assert.equal(finished.outcome.interrupts[0].reason, "tool_call");
    assert.ok(!fs.existsSync(path.join(srv.workspace, file)), "lo strumento è partito prima della risposta");
    const resume = [{ interruptId: finished.outcome.interrupts[0].id, status: "resolved", payload: { approved: true } }];
    const second = await run("operator", { threadId, runId: uid("run"), messages, forwardedProps, resume });
    assert.equal(second.types.at(-1), "RUN_FINISHED", `${second.types}`);
    assert.ok(second.types.includes("TOOL_CALL_RESULT") && !second.types.includes("TOOL_CALL_START"), `${second.types}`);
    assert.equal(fs.readFileSync(path.join(srv.workspace, file), "utf8"), "ok");
    assert.equal(second.events.filter((e) => e.type === "TEXT_MESSAGE_CONTENT").map((e) => e.delta).join(""), "Scritto");
  });
});
