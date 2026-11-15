// Prova di interoperabilità AG-UI: il vero `@ag-ui/client` (`HttpAgent`) contro il vero `aiw serve`, con un provider OpenAI finto
// al posto del modello. Il client controlla ogni evento (ordine, coppie START/END, forma): uno stream fuori norma lo fa fallire.
// Gli scenari di base girano con due versioni del client: la 1.0.4 (ultima) e la 1.0.2 (quella di `@copilotkit/runtime` 1.88.0).
//
// Che cosa NON prova: CopilotKit stesso (React, runtime con il suo adattatore) né un modello vero. Vedi `docs/agui.md`.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { after, afterEach, before, describe, it } from "node:test";
import { call, calls, startFakeProvider, text } from "../lib/fake-provider.mjs";
import { assistantTexts, loadClients, makeAgent, rawRun, runCollect, sleep, uid, until, userMessage } from "../lib/client.mjs";
import { startServer } from "../lib/server.mjs";

// Il client avvisa con `console.warn` quando toglie campi che non conosce o non riesce ad applicare una patch dello stato:
// per questa prova sono difetti dello stream, e ogni test deve chiudere senza avvisi.
const warnings = [];
const realWarn = console.warn;
console.warn = (...args) => {
  warnings.push(args.map(String).join(" "));
};

const clients = await loadClients();
const [latest] = clients;
const INTERRUPTS = { aiw: { interactive: ["interrupts"] } };

let prov;
let srv;
afterEach((t) => {
  const seen = warnings.splice(0).filter((w) => /ag-ui\]\[enforce|Failed to apply state patch|Removed unrecognised/i.test(w));
  assert.deepEqual(seen, [], `il client ha avvisato durante «${t.name}»`);
});
before(async () => {
  prov = await startFakeProvider();
  srv = await startServer({ providerBase: prov.base });
});
after(async () => {
  console.warn = realWarn;
  await srv?.stop();
  await prov?.close();
});

const ok = (r) => assert.equal(r.failure, undefined, `il client ha rifiutato lo stream: ${r.failure?.message ?? r.failure}`);
const sessionByTitle = async (title) => (await srv.api("GET", "/api/sessions")).json.find((s) => s.title === title);
const storedMessages = async (title) => (await srv.api("GET", `/api/sessions/${(await sessionByTitle(title)).id}/messages`)).json;
const approvalsOf = async (file) => ((await srv.api("GET", "/api/approvals")).json ?? []).filter((a) => a.args?.path === file);
const written = (file) => path.join(srv.workspace, file);
const b64 = (s) => Buffer.from(s).toString("base64");

for (const Client of clients) {
  describe(Client.name, () => {
    it("testo con ragionamento: il client accetta lo stream, tiene il suo stato e il modello vede il contesto", async () => {
      const tag = uid("t").slice(0, 12);
      prov.set([text("Ciao dal provider finto", "Penso un attimo.")]);
      const agent = makeAgent(Client, srv, "planner", { messages: [userMessage(`Saluta ${tag}`)], state: { tema: "scuro" } });
      const r = await runCollect(agent, { runId: uid("run"), context: [{ description: "Pagina corrente", value: `Fattura ${tag}` }] });
      ok(r);
      assert.equal(r.types[0], "RUN_STARTED");
      assert.equal(r.types.at(-1), "RUN_FINISHED");
      const order = ["REASONING_START", "REASONING_MESSAGE_START", "REASONING_MESSAGE_CONTENT", "REASONING_MESSAGE_END", "REASONING_END", "TEXT_MESSAGE_START"];
      assert.deepEqual(order.map((t) => r.types.indexOf(t)), order.map((t) => r.types.indexOf(t)).toSorted((a, b) => a - b), `ordine: ${r.types}`);
      assert.deepEqual(assistantTexts(agent), ["Ciao dal provider finto"]);
      // Lo stato del client resta; quello del motore sta sotto `aiw`.
      assert.equal(agent.state.tema, "scuro");
      assert.equal(agent.state.aiw.model, "fake-model");
      assert.ok(r.types.includes("STATE_SNAPSHOT") && r.types.includes("STATE_DELTA"), `${r.types}`);
      // Il contesto va al modello come dati del solo run: non resta nella cronologia.
      const system = prov.requests.at(-1).messages[0];
      assert.equal(system.role, "system");
      assert.ok(system.content.includes(`Fattura ${tag}`) && system.content.includes("non istruzioni"), "il contesto non è nel prompt");
      assert.ok(system.content.includes('"tema": "scuro"'), "lo stato condiviso non è nel prompt");
      const stored = await storedMessages(`Saluta ${tag}`);
      assert.ok(!JSON.stringify(stored).includes(`Fattura ${tag}`), "il contesto è finito nella cronologia");
    });

    it("strumento e piano: i messaggi si ricostruiscono e lo stato porta il piano", async () => {
      prov.set([call("todo", { todos: [{ content: "primo", status: "pending" }] }), text("Piano pronto")]);
      const agent = makeAgent(Client, srv, "planner", { messages: [userMessage(`Pianifica ${uid("t").slice(0, 8)}`)] });
      const r = await runCollect(agent, { runId: uid("run") });
      ok(r);
      assert.deepEqual(agent.messages.map((m) => m.role), ["user", "assistant", "tool", "assistant"]);
      assert.equal(agent.messages[1].toolCalls[0].function.name, "todo");
      assert.equal(agent.messages[2].toolCallId, agent.messages[1].toolCalls[0].id);
      assert.equal(agent.state.aiw.todos.length, 1);
    });

    it("sotto-agente: SUBAGENT_STARTED e SUBAGENT_FINISHED con subagentRunId", async () => {
      prov.set([call("agent", { agent: "planner", task: "ciao" }), text("risposta del sotto-agente"), text("Finito")]);
      const agent = makeAgent(Client, srv, "operator", { messages: [userMessage(`Delega ${uid("t").slice(0, 8)}`)] });
      const r = await runCollect(agent, { runId: uid("run") });
      ok(r);
      const started = r.events.find((e) => e.type === "SUBAGENT_STARTED");
      const finished = r.events.find((e) => e.type === "SUBAGENT_FINISHED");
      assert.ok(started?.subagentRunId && started.subagentRunId === finished?.subagentRunId, JSON.stringify([started, finished]));
      assert.equal(started.name, "planner");
      assert.equal(assistantTexts(agent).at(-1), "Finito");
    });

    it("allegati: documento e immagine con dati inline diventano allegati del messaggio e il modello legge il testo", async () => {
      const tag = uid("t").slice(0, 10);
      prov.set([text("Letto")]);
      const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==", "base64");
      const content = [
        { type: "text", text: `Leggi ${tag}` },
        { type: "document", source: { type: "data", value: b64("# Nota\nIl codice è ZETA-42"), mimeType: "text/markdown" }, metadata: { filename: "nota.md" } },
        { type: "image", source: { type: "data", value: png.toString("base64"), mimeType: "image/png" }, metadata: { filename: "punto.png" } },
      ];
      const agent = makeAgent(Client, srv, "planner", { messages: [userMessage(content)] });
      const r = await runCollect(agent, { runId: uid("run") });
      ok(r);
      const stored = await storedMessages(`Leggi ${tag}`);
      const atts = stored.find((m) => m.role === "user").attachments;
      assert.deepEqual(atts.map((a) => [a.name, a.mime, a.kind]), [["nota.md", "text/markdown", "text"], ["punto.png", "image/png", "image"]]);
      // Il testo del documento è arrivato al modello; i byte dell'immagine non sono nella cronologia salvata.
      assert.ok(JSON.stringify(prov.requests.at(-1).messages).includes("ZETA-42"), "il modello non ha letto il documento");
      assert.ok(!JSON.stringify(stored).includes(png.toString("base64")));
    });

    it("conferma come CUSTOM aiw.approval_request: il run aspetta la risposta su REST e poi finisce", async () => {
      const file = `custom-${uid("f").slice(0, 8)}.md`;
      prov.set([call("write_file", { path: file, content: "ciao" }), text("Scritto")]);
      const agent = makeAgent(Client, srv, "operator", { messages: [userMessage(`Scrivi ${file}`)] });
      const running = runCollect(agent, { runId: uid("run") });
      const [ap] = await until("la conferma è aperta", async () => {
        const a = await approvalsOf(file);
        return a.length ? a : null;
      });
      assert.ok(!fs.existsSync(written(file)), "lo strumento è partito senza conferma");
      assert.equal((await srv.api("POST", `/api/approvals/${ap.id}`, { decision: "allow" })).status, 200);
      const r = await running;
      ok(r);
      const customs = r.events.filter((e) => e.type === "CUSTOM").map((e) => e.name);
      assert.deepEqual(customs.filter((n) => n.startsWith("aiw.approval")), ["aiw.approval_request", "aiw.approval_resolved"]);
      assert.equal(fs.readFileSync(written(file), "utf8"), "ciao");
      assert.equal(assistantTexts(agent).at(-1), "Scritto");
    });

    it("conferma come interruzione: RUN_FINISHED con outcome interrupt, poi resume riprende lo stesso run", async () => {
      const file = `resume-${uid("f").slice(0, 8)}.md`;
      const n0 = prov.requests.length;
      prov.set([call("write_file", { path: file, content: "approvato" }), text("Scritto")]);
      const agent = makeAgent(Client, srv, "operator", { messages: [userMessage(`Scrivi ${file}`)] });
      const run1 = uid("run");
      const r1 = await runCollect(agent, { runId: run1, forwardedProps: INTERRUPTS });
      ok(r1);
      assert.equal(r1.outcome?.type, "interrupt", JSON.stringify(r1.outcome));
      const [it] = r1.outcome.interrupts;
      const carrier = agent.messages.find((m) => m.role === "assistant" && m.toolCalls?.length);
      assert.equal(it.reason, "tool_call");
      assert.equal(it.toolCallId, carrier.toolCalls[0].id, "l'interruzione non è legata alla chiamata dello strumento");
      assert.deepEqual(it.responseSchema.required, ["approved"]);
      assert.equal(it.metadata["ag-ui:tool-execution"], "server");
      assert.equal(it.metadata.aiw.tool, "write_file");
      // La specifica: prima dell'interruzione vanno gli snapshot dello stato e dei messaggi.
      const finish = r1.types.lastIndexOf("RUN_FINISHED");
      assert.ok(r1.types.lastIndexOf("MESSAGES_SNAPSHOT") < finish && r1.types.lastIndexOf("STATE_SNAPSHOT") < finish, `${r1.types}`);
      assert.ok(!fs.existsSync(written(file)), "lo strumento è partito prima della risposta");
      assert.equal((await approvalsOf(file)).length, 1, "la conferma è ancora aperta nel motore");

      const run2 = uid("run");
      const r2 = await runCollect(agent, { runId: run2, forwardedProps: INTERRUPTS, resume: [{ interruptId: it.id, status: "resolved", payload: { approved: true } }] });
      ok(r2);
      assert.equal(r2.outcome, undefined);
      assert.equal(r2.events[0].type, "RUN_STARTED");
      assert.equal(r2.events[0].runId, run2);
      assert.equal(r2.events[0].parentRunId, run1);
      assert.ok(!r2.types.includes("TOOL_CALL_START"), "la chiamata non va ripetuta alla ripresa (spec: solo il risultato)");
      assert.ok(r2.types.includes("TOOL_CALL_RESULT") && r2.types.at(-1) === "RUN_FINISHED", `${r2.types}`);
      assert.equal(fs.readFileSync(written(file), "utf8"), "approvato");
      assert.deepEqual(agent.messages.map((m) => m.role), ["user", "assistant", "tool", "assistant"]);
      assert.equal(assistantTexts(agent).at(-1), "Scritto");
      assert.equal(prov.requests.length - n0, 2, "il modello è stato chiamato due volte: prima e dopo lo strumento, non di più");
    });
  });
}

describe("human-in-the-loop interoperabile (@ag-ui/client 1.0.4)", () => {
  it("un rifiuto e un annullamento non eseguono lo strumento e il modello va avanti", async () => {
    const [a, b] = [`no-${uid("f").slice(0, 8)}.md`, `ann-${uid("f").slice(0, 8)}.md`];
    for (const [file, answer] of [[a, { status: "resolved", payload: { approved: false } }], [b, { status: "cancelled" }]]) {
      prov.set([call("write_file", { path: file, content: "x" }), text("Va bene, non scrivo")]);
      const agent = makeAgent(latest, srv, "operator", { messages: [userMessage(`Scrivi ${file}`)] });
      const r1 = await runCollect(agent, { runId: uid("run"), forwardedProps: INTERRUPTS });
      const r2 = await runCollect(agent, { runId: uid("run"), forwardedProps: INTERRUPTS, resume: [{ interruptId: r1.outcome.interrupts[0].id, ...answer }] });
      ok(r2);
      assert.ok(!fs.existsSync(written(file)), `${file} è stato scritto`);
      assert.equal(assistantTexts(agent).at(-1), "Va bene, non scrivo");
    }
  });

  it("le regole del resume: id noti, risposta valida, niente input nuovo, replay sicuro, risposta diversa è un conflitto", async () => {
    const file = `regole-${uid("f").slice(0, 8)}.md`;
    const n0 = prov.requests.length;
    prov.set([call("write_file", { path: file, content: "a" }), text("Fatto")]);
    const agent = makeAgent(latest, srv, "operator", { messages: [userMessage(`Scrivi ${file}`)] });
    const r1 = await runCollect(agent, { runId: uid("run"), forwardedProps: INTERRUPTS });
    ok(r1);
    const [it] = r1.outcome.interrupts;
    const atInterrupt = structuredClone(agent.messages);
    const resolved = { interruptId: it.id, status: "resolved", payload: { approved: true } };
    // Il client vero non manda un resume che non risponde a tutte le interruzioni aperte (lo rifiuta da sé): questi casi
    // si scrivono a mano, perché è il server a doverli rifiutare.
    const wrong = async (extra, code, messages = atInterrupt) => {
      const r = await rawRun(srv, "operator", { threadId: agent.threadId, runId: uid("run"), messages, forwardedProps: INTERRUPTS, ...extra });
      assert.deepEqual(r.events.map((e) => e.type), ["RUN_STARTED", "RUN_ERROR"], r.raw);
      assert.equal(r.events[1].code, code, r.raw);
      assert.equal((await approvalsOf(file)).length, 1, "un rifiuto del protocollo ha toccato la conferma");
      assert.ok(!fs.existsSync(written(file)));
    };
    await wrong({ resume: [{ ...resolved, interruptId: "non-esiste" }] }, "interrupt_not_found");
    await wrong({ resume: [{ ...resolved, payload: {} }] }, "resume_invalid");
    await wrong({ resume: [{ ...resolved, status: "forse" }] }, "resume_invalid");
    // Altro input mentre l'interruzione è aperta (la specifica: RUN_ERROR, non un nuovo run), e un nuovo messaggio insieme alla risposta.
    await wrong({}, "interrupt_pending", [...atInterrupt, userMessage("e adesso un'altra cosa")]);
    await wrong({ resume: [resolved] }, "resume_with_new_input", [...atInterrupt, userMessage("e adesso un'altra cosa")]);

    const r2 = await runCollect(agent, { runId: uid("run"), forwardedProps: INTERRUPTS, resume: [resolved] });
    ok(r2);
    assert.equal(fs.readFileSync(written(file), "utf8"), "a");
    const calledTwice = prov.requests.length - n0;
    assert.equal(calledTwice, 2);

    // Un client che ha perso la risposta rimanda la stessa risposta: stesso esito, nessuna nuova chiamata, nessun effetto doppio.
    const lost = makeAgent(latest, srv, "operator", { threadId: agent.threadId, messages: structuredClone(atInterrupt) });
    const r3 = await runCollect(lost, { runId: uid("run"), forwardedProps: INTERRUPTS, resume: [resolved] });
    ok(r3);
    assert.equal(r3.types.at(-1), "RUN_FINISHED");
    assert.equal(assistantTexts(lost).at(-1), "Fatto");
    assert.equal(prov.requests.length - n0, calledTwice, "il replay ha richiamato il modello");
    // Un'altra risposta alla stessa interruzione è un conflitto.
    const other = makeAgent(latest, srv, "operator", { threadId: agent.threadId, messages: structuredClone(atInterrupt) });
    const r4 = await runCollect(other, { runId: uid("run"), forwardedProps: INTERRUPTS, resume: [{ ...resolved, payload: { approved: false } }] });
    assert.equal(r4.runError?.code, "resume_conflict", JSON.stringify(r4.runError ?? r4.types));
  });

  it("una catena di conferme: ogni pausa è un RUN_FINISHED con la sua interruzione e ogni resume ha il parentRunId del run precedente", async () => {
    const [a, b] = [`cat-a-${uid("f").slice(0, 8)}.md`, `cat-b-${uid("f").slice(0, 8)}.md`];
    prov.set([calls([["write_file", { path: a, content: "a" }], ["write_file", { path: b, content: "b" }]]), text("Tutti e due")]);
    const agent = makeAgent(latest, srv, "operator", { messages: [userMessage(`Scrivi ${a} e ${b}`)] });
    const run1 = uid("run");
    const r1 = await runCollect(agent, { runId: run1, forwardedProps: INTERRUPTS });
    ok(r1);
    // Il motore chiede le conferme una alla volta: la seconda arriva quando la prima ha avuto risposta.
    assert.equal(r1.outcome.interrupts.length, 1);
    const run2 = uid("run");
    const r2 = await runCollect(agent, { runId: run2, forwardedProps: INTERRUPTS, resume: [{ interruptId: r1.outcome.interrupts[0].id, status: "resolved", payload: { approved: true } }] });
    ok(r2);
    assert.equal(r2.events[0].parentRunId, run1);
    assert.equal(r2.outcome?.type, "interrupt", `${r2.types}`);
    assert.equal(r2.outcome.interrupts.length, 1);
    assert.notEqual(r2.outcome.interrupts[0].id, r1.outcome.interrupts[0].id);
    assert.ok(fs.existsSync(written(a)) && !fs.existsSync(written(b)));
    const run3 = uid("run");
    const r3 = await runCollect(agent, { runId: run3, forwardedProps: INTERRUPTS, resume: [{ interruptId: r2.outcome.interrupts[0].id, status: "resolved", payload: { approved: true } }] });
    ok(r3);
    assert.equal(r3.events[0].parentRunId, run2);
    assert.equal(r3.outcome, undefined);
    assert.ok(fs.existsSync(written(b)));
    assert.equal(assistantTexts(agent).at(-1), "Tutti e due");
    assert.deepEqual(agent.messages.map((m) => m.role), ["user", "assistant", "tool", "tool", "assistant"]);
  });

  it("resume senza interruzione aperta (thread ignoto) è RUN_ERROR e non lascia sessioni", async () => {
    const before = (await srv.api("GET", "/api/sessions")).json.length;
    const agent = makeAgent(latest, srv, "operator", { messages: [userMessage("ciao")] });
    const r = await runCollect(agent, { runId: uid("run"), resume: [{ interruptId: "x", status: "resolved", payload: { approved: true } }] });
    assert.equal(r.runError?.code, "interrupt_not_found");
    assert.equal((await srv.api("GET", "/api/sessions")).json.length, before);
  });

  it("una domanda di ask_user è un'interruzione input_required con lo schema della risposta", async () => {
    const tag = uid("q").slice(0, 8);
    const question = {
      kind: "choice", title: "Quale modello?", message: `Ho trovato due varianti ${tag}.`,
      options: [{ id: "q4", label: "Q4_K_M", description: "Consigliata" }, { id: "q8", label: "Q8_0" }],
    };
    prov.set([call("ask_user", question), text("Scarico la Q4.")]);
    const agent = makeAgent(latest, srv, "assistant", { messages: [userMessage(`Scegli un modello ${tag}`)] });
    const r1 = await runCollect(agent, { runId: uid("run"), forwardedProps: INTERRUPTS });
    ok(r1);
    const [it] = r1.outcome.interrupts;
    assert.equal(it.reason, "input_required");
    assert.deepEqual(it.responseSchema.properties.selected.items.enum, ["q4", "q8"]);
    assert.equal(it.metadata.aiw.question.kind, "choice");
    assert.ok(it.message.includes("Quale modello?") && it.message.includes(tag));
    assert.equal(it.toolCallId, agent.messages.find((m) => m.toolCalls?.length).toolCalls[0].id);
    // Una risposta che non va con la domanda è un errore e la domanda resta aperta.
    const bad = await runCollect(agent, { runId: uid("run"), forwardedProps: INTERRUPTS, resume: [{ interruptId: it.id, status: "resolved", payload: { selected: ["zzz"] } }] });
    assert.equal(bad.runError?.code, "resume_invalid", JSON.stringify(bad.runError ?? bad.types));
    assert.equal(((await srv.api("GET", "/api/questions")).json ?? []).length, 1);
    const r2 = await runCollect(agent, { runId: uid("run"), forwardedProps: INTERRUPTS, resume: [{ interruptId: it.id, status: "resolved", payload: { selected: ["q4"] } }] });
    ok(r2);
    const result = JSON.parse(agent.messages.find((m) => m.role === "tool").content);
    assert.deepEqual([result.outcome, result.selected], ["answered", ["q4"]]);
    assert.equal(assistantTexts(agent).at(-1), "Scarico la Q4.");
  });

  it("la conferma di uno strumento di gestione dell'Assistente è un'interruzione tool_call e il proprietario la decide con resume", async () => {
    prov.set([call("aiw_setting_set", { key: "ui.theme", value: "dark" }), text("Lascio il tema com'è")]);
    const agent = makeAgent(latest, srv, "assistant", { messages: [userMessage(`Metti il tema scuro ${uid("t").slice(0, 6)}`)] });
    const r1 = await runCollect(agent, { runId: uid("run"), forwardedProps: INTERRUPTS });
    ok(r1);
    const [it] = r1.outcome.interrupts;
    assert.equal(it.reason, "tool_call");
    assert.equal(it.metadata.aiw.tool, "aiw_setting_set");
    // Dal computer stesso è il proprietario. Il rifiuto da un token `device` (403, la pausa resta) è provato nei test Rust
    // (`cargo test -p aiw-server agui`): qui non c'è un dispositivo remoto.
    const r2 = await runCollect(agent, { runId: uid("run"), forwardedProps: INTERRUPTS, resume: [{ interruptId: it.id, status: "resolved", payload: { approved: false } }] });
    ok(r2);
    assert.equal(assistantTexts(agent).at(-1), "Lascio il tema com'è");
  });
});

describe("ripresa, riconnessione e thread durevole (@ag-ui/client 1.0.4)", () => {
  it("lo stesso messaggio rimandato con un altro runId rimanda il run già fatto, senza rifarlo", async () => {
    const tag = uid("r").slice(0, 10);
    const msg = userMessage(`Saluta ${tag}`);
    prov.set([text("Risposta unica")]);
    const threadId = uid("thr");
    const a1 = makeAgent(latest, srv, "planner", { threadId, messages: [msg] });
    ok(await runCollect(a1, { runId: uid("run") }));
    const n = prov.requests.length;
    // Il client ha perso la risposta (una pagina ricaricata, la rete): rimanda lo stesso messaggio con un runId nuovo.
    const run2 = uid("run");
    const a2 = makeAgent(latest, srv, "planner", { threadId, messages: [structuredClone(msg)] });
    const r2 = await runCollect(a2, { runId: run2 });
    ok(r2);
    assert.equal(r2.events[0].runId, run2);
    assert.ok(r2.types.includes("MESSAGES_SNAPSHOT") && r2.types.at(-1) === "RUN_FINISHED", `${r2.types}`);
    assert.deepEqual(assistantTexts(a2), ["Risposta unica"]);
    assert.equal(a2.messages.filter((m) => m.role === "user").length, 1, "il messaggio dell'utente è doppio");
    assert.equal(prov.requests.length, n, "il run è stato rifatto");
    assert.equal((await storedMessages(`Saluta ${tag}`)).length, 2, "la cronologia ha messaggi doppi");
    // Un messaggio nuovo con lo stesso id (un client che manda sempre «m1») non è il vecchio: parte un run nuovo.
    prov.set([text("Seconda risposta")]);
    const a3 = makeAgent(latest, srv, "planner", { threadId, messages: [{ ...msg, content: `Saluta ${tag} ancora` }] });
    ok(await runCollect(a3, { runId: uid("run") }));
    assert.equal(prov.requests.length, n + 1);
    assert.equal(assistantTexts(a3).at(-1), "Seconda risposta");
  });

  it("un client senza cronologia (pagina ricaricata) riceve quella del thread con MESSAGES_SNAPSHOT", async () => {
    const tag = uid("h").slice(0, 10);
    const threadId = uid("thr");
    const first = makeAgent(latest, srv, "planner", { threadId, messages: [userMessage(`Primo ${tag}`)] });
    prov.set([text("Uno")]);
    ok(await runCollect(first, { runId: uid("run") }));
    prov.set([text("Due")]);
    first.addMessage(userMessage("Secondo"));
    ok(await runCollect(first, { runId: uid("run") }));
    assert.deepEqual(assistantTexts(first), ["Uno", "Due"]);
    // Un client nuovo sullo stesso thread, con il solo messaggio nuovo.
    const fresh = makeAgent(latest, srv, "planner", { threadId, messages: [userMessage("Terzo")] });
    prov.set([text("Tre")]);
    const r = await runCollect(fresh, { runId: uid("run") });
    ok(r);
    assert.ok(r.types.includes("MESSAGES_SNAPSHOT"), `${r.types}`);
    assert.deepEqual(fresh.messages.map((m) => m.role), ["user", "assistant", "user", "assistant", "user", "assistant"]);
    assert.deepEqual(assistantTexts(fresh), ["Uno", "Due", "Tre"]);
    assert.deepEqual(fresh.messages.filter((m) => m.role === "assistant").slice(0, 2).map((m) => m.id), first.messages.filter((m) => m.role === "assistant").map((m) => m.id), "gli id dei messaggi dell'assistente sono quelli del server");
    assert.equal(new Set(fresh.messages.map((m) => m.id)).size, fresh.messages.length, "id duplicati");
    // Un client che ha già la cronologia non ne riceve una seconda.
    first.addMessage(userMessage("Quarto"));
    prov.set([text("Quattro")]);
    const again = await runCollect(first, { runId: uid("run") });
    ok(again);
    assert.ok(!again.types.includes("MESSAGES_SNAPSHOT"), "uno snapshot a un client già allineato");
  });

  it("chiudere la richiesta annulla il run (AbortController del client); una pausa invece non lo annulla", async () => {
    const file = `abort-${uid("f").slice(0, 8)}.md`;
    prov.set([call("write_file", { path: file, content: "x" }), text("mai")]);
    const agent = makeAgent(latest, srv, "operator", { messages: [userMessage(`Scrivi ${file}`)] });
    const running = runCollect(agent, { runId: uid("run") });
    const [ap] = await until("la conferma è aperta", async () => {
      const a = await approvalsOf(file);
      return a.length ? a : null;
    });
    agent.abortRun();
    await running;
    const events = await until("il run è chiuso", async () => {
      const ev = (await srv.api("GET", `/api/runs/${ap.run_id}/events.json`)).json ?? [];
      const last = ev.at(-1)?.event ?? ev.at(-1);
      return last && ["RUN_ERROR", "RUN_FINISHED"].includes(last.type) ? ev : null;
    });
    const last = events.at(-1)?.event ?? events.at(-1);
    assert.equal(last.type, "RUN_ERROR", "annullato, non finito");
    assert.equal((await approvalsOf(file)).length, 0, "la conferma è rimasta aperta");
    assert.ok(!fs.existsSync(written(file)));
    // La pausa: il client chiude la connessione dopo RUN_FINISHED (interrupt) e il run resta in attesa.
    const file2 = `pausa-${uid("f").slice(0, 8)}.md`;
    prov.set([call("write_file", { path: file2, content: "y" }), text("Scritto")]);
    const a2 = makeAgent(latest, srv, "operator", { messages: [userMessage(`Scrivi ${file2}`)] });
    ok(await runCollect(a2, { runId: uid("run"), forwardedProps: INTERRUPTS }));
    await sleep(300);
    assert.equal((await approvalsOf(file2)).length, 1, "la pausa ha annullato il run");
  });

  it("i segreti non compaiono mai nello stream, nei messaggi salvati né nelle richieste al modello", async () => {
    const FAKE = "FAKE_BRAVE_KEY_e2e_7d6c5b4a3f";
    const tag = uid("s").slice(0, 8);
    prov.set([call("aiw_secret_request", { target: "setting:search.brave_api_key", label: "Chiave di Brave", reason: "Serve per cercare sul web" }), text("Chiave salvata.")]);
    const agent = makeAgent(latest, srv, "assistant", { messages: [userMessage(`Imposta la chiave di Brave ${tag}`)] });
    const running = runCollect(agent, { runId: uid("run"), forwardedProps: { aiw: { interactive: ["questions", "secrets"] } } });
    const req = await until("la richiesta del segreto è aperta", async () => ((await srv.api("GET", "/api/secret-requests")).json ?? [])[0]);
    assert.deepEqual(Object.keys(req).toSorted(), ["destination", "expires_at", "id", "label", "reason", "run_id", "target"]);
    const stored = await srv.api("POST", `/api/secret-requests/${req.id}`, { action: "provide", value: FAKE });
    assert.deepEqual([stored.status, stored.json], [200, { stored: true }]);
    const r = await running;
    ok(r);
    assert.ok(r.events.some((e) => e.type === "CUSTOM" && e.name === "aiw.secret_request"));
    const everything = JSON.stringify([r.events, agent.messages, prov.requests, await storedMessages(`Imposta la chiave di Brave ${tag}`), srv.log()]);
    assert.ok(!everything.includes(FAKE), "il segreto è finito in un evento, un messaggio, una richiesta al modello o un log");
    assert.equal(assistantTexts(agent).at(-1), "Chiave salvata.");
  });

  it("gli strumenti del client e parentRunId sono accettati e ignorati: il modello non li vede", async () => {
    prov.set([text("Ok")]);
    const agent = makeAgent(latest, srv, "planner", { messages: [userMessage(`Ciao ${uid("t").slice(0, 6)}`)] });
    const r = await runCollect(agent, {
      runId: uid("run"),
      tools: [{ name: "mostra_grafico", description: "Mostra un grafico", parameters: { type: "object", properties: {} } }],
    });
    ok(r);
    const sent = JSON.stringify(prov.requests.at(-1).tools ?? []);
    assert.ok(!sent.includes("mostra_grafico"), "uno strumento del client è arrivato al modello");
  });

  it("dopo un riavvio il thread continua nella stessa sessione e un messaggio già eseguito non si rifà", async () => {
    const tag = uid("d").slice(0, 10);
    const threadId = uid("thr");
    const msg = userMessage(`Ricorda ${tag}`);
    prov.set([text("Annotato")]);
    const a1 = makeAgent(latest, srv, "planner", { threadId, messages: [msg] });
    ok(await runCollect(a1, { runId: uid("run") }));
    const sessions = (await srv.api("GET", "/api/sessions")).json.length;
    await srv.restart();
    // Lo stesso messaggio, dopo il riavvio: si rimanda il run, senza chiamare il modello.
    const n = prov.requests.length;
    const a2 = makeAgent(latest, srv, "planner", { threadId, messages: [structuredClone(msg)] });
    const r2 = await runCollect(a2, { runId: uid("run") });
    ok(r2);
    assert.deepEqual(assistantTexts(a2), ["Annotato"]);
    assert.equal(prov.requests.length, n, "il run è stato rifatto dopo il riavvio");
    // Un messaggio nuovo continua la stessa sessione, con la cronologia di prima.
    prov.set([text("Ricordo")]);
    const a3 = makeAgent(latest, srv, "planner", { threadId, messages: [userMessage("Che cosa ti ho detto?")] });
    const r3 = await runCollect(a3, { runId: uid("run") });
    ok(r3);
    assert.equal((await srv.api("GET", "/api/sessions")).json.length, sessions, "il riavvio ha fatto nascere un'altra sessione");
    assert.deepEqual(assistantTexts(a3), ["Annotato", "Ricordo"]);
    const stored = await storedMessages(`Ricorda ${tag}`);
    assert.deepEqual(stored.filter((m) => m.role === "user").map((m) => m.content), [`Ricorda ${tag}`, "Che cosa ti ho detto?"]);
  });
});
