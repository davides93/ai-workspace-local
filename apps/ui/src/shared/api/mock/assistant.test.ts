import { describe, expect, it } from "vitest";
import { ApiError } from "../http";
import type { AgEvent, ApprovalRequest, Question, SecretRequest } from "../types";
import { historyToItems } from "../../../features/chat/model/history";
import type { QuestionItem, SecretItem } from "../../../features/chat/model/items";
import { reduce } from "../../../features/chat/model/reduce";
import { assistantScript, checkAnswer } from "./assistant";
import * as fixtures from "./fixtures";
import { MockTransport } from "./index";

/** A token that is obviously not one: any trace of it anywhere is a leak. */
const FAKE_TOKEN = "hf_TEST_ONLY_not_a_real_token_0000";

type Custom = Extract<AgEvent, { type: "CUSTOM" }>;
const custom = (name: string) => (e: AgEvent): e is Custom => e.type === "CUSTOM" && e.name === name;

/** Resolves with the first event (past or future) of a run that fits. */
function waitFor<T extends AgEvent>(api: MockTransport, runId: string, fits: (e: AgEvent) => e is T): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("event not seen")), 8000);
    let done = false;
    // The events already emitted are replayed before `subscribeRun` returns: `off` is not there yet for them.
    let off: (() => void) | undefined;
    const stop = api.subscribeRun(runId, (e) => {
      if (done || !fits(e)) return;
      done = true;
      clearTimeout(timer);
      queueMicrotask(() => off?.());
      resolve(e);
    });
    off = stop;
    if (done) queueMicrotask(stop);
  });
}
const finished = (e: AgEvent): e is Extract<AgEvent, { type: "RUN_FINISHED" }> => e.type === "RUN_FINISHED";
const failed = (e: AgEvent): e is Extract<AgEvent, { type: "RUN_ERROR" }> => e.type === "RUN_ERROR";

async function start(api: MockTransport, text: string) {
  const session = await api.createSession({ agent_id: "operator" });
  const { run_id } = await api.startRun(session.id, text);
  return { session, runId: run_id };
}

/** Every string reachable from `root` (Maps, Sets and arrays included), to look for a value that must not be there. */
function reachableText(root: unknown): string {
  const out: string[] = [];
  const seen = new Set<unknown>();
  const walk = (v: unknown): void => {
    if (typeof v === "string") return void out.push(v);
    if (!v || typeof v !== "object" || seen.has(v)) return;
    seen.add(v);
    if (v instanceof Map) for (const [k, x] of v) { walk(k); walk(x); }
    else if (v instanceof Set) for (const x of v) walk(x);
    else for (const k of Reflect.ownKeys(v)) walk((v as Record<PropertyKey, unknown>)[k]);
  };
  walk(root);
  return out.join("\n");
}

describe("scripts of the Assistente (demo)", () => {
  it("start from their phrases, in any session, without case or accents", () => {
    expect(assistantScript("Scarica un modello")).toBe("download");
    expect(assistantScript("per favore, SCARICA UN MODELLO adesso")).toBe("download");
    expect(assistantScript("Aggiungi il token di Hugging Face")).toBe("secret");
    expect(assistantScript("Quali modelli confrontiamo?")).toBe("multiple");
    expect(assistantScript("Cancella i download vecchi")).toBe("confirm");
    expect(assistantScript("Crea un assistente per le email")).toBe("form");
    expect(assistantScript("Ordina i PDF in Downloads")).toBeNull();
    expect(assistantScript("")).toBeNull();
  });

  it("«Scarica un modello»: a single-choice question whose events have the shape of the contract; the answer lets the run go on", async () => {
    const api = new MockTransport();
    const { session, runId } = await start(api, "Scarica un modello");
    const opened = await waitFor(api, runId, custom("aiw.question"));
    const q = opened.value as Question;
    expect(Object.keys(q).sort()).toEqual(["allow_other", "confirm_label", "deny_label", "expires_at", "fields", "id", "kind", "message", "multiple", "options", "run_id", "title"]);
    expect(q).toMatchObject({ run_id: runId, kind: "choice", multiple: false, allow_other: false, fields: [], confirm_label: null, deny_label: null });
    expect(q.options.map((o) => o.id)).toEqual(["q4", "q5", "q8"]);
    expect(q.options.every((o) => "description" in o)).toBe(true);
    expect(Date.parse(q.expires_at ?? "")).toBeGreaterThan(Date.now());
    // It waits, and the list says so.
    expect(await api.questions()).toEqual([q]);

    // A wrong answer is refused and the question stays open.
    await expect(api.answerQuestion(q.id, { action: "answer", selected: ["nope"], other: null, confirmed: null, values: null })).rejects.toMatchObject({ status: 400 });
    await expect(api.answerQuestion(q.id, { action: "answer", selected: ["q4", "q8"], other: null, confirmed: null, values: null })).rejects.toMatchObject({ status: 400 });
    expect(await api.questions()).toHaveLength(1);

    await api.answerQuestion(q.id, { action: "answer", selected: ["q4"], other: null, confirmed: null, values: null });
    const resolved = await waitFor(api, runId, custom("aiw.question_resolved"));
    expect(resolved.value).toEqual({ id: q.id, outcome: "answered", answer: { selected: ["q4"], other: null, confirmed: null, values: null } });
    expect(await api.questions()).toEqual([]);
    await expect(api.answerQuestion(q.id, { action: "decline" })).rejects.toMatchObject({ status: 404 });

    // The run goes on: a management tool asks for its confirmation, and then the run finishes.
    const approval = await waitFor(api, runId, custom("aiw.approval_request"));
    const req = approval.value as ApprovalRequest;
    expect(req.tool).toBe("aiw_model_download");
    await api.respondApproval(req.id, "allow");
    await waitFor(api, runId, finished);

    // The stored conversation: the call and its result as the model got them, then the final text.
    const msgs = await api.messages(session.id);
    const result = msgs.find((m) => m.role === "tool" && m.content.includes('"outcome":"answered"'));
    expect(JSON.parse(result?.content ?? "{}")).toEqual({ outcome: "answered", selected: ["q4"], other: null, confirmed: null, values: null });
    expect(msgs.find((m) => m.tool_calls?.[0]?.name === "ask_user")?.tool_calls?.[0].id).toBe(result?.tool_call_id);
    // …and reloading it shows the question as a closed row, the same one the live events produced.
    const reloaded = historyToItems(msgs).find((i): i is QuestionItem => i.kind === "question");
    expect(reloaded).toMatchObject({ outcome: "answered", answer: { selected: ["q4"] }, q: { message: q.message } });
    const live = (await api.runEvents(runId)).reduce((items, e) => reduce(items, e), [] as ReturnType<typeof historyToItems>).find((i): i is QuestionItem => i.kind === "question");
    expect(live).toMatchObject({ outcome: "answered", answer: { selected: ["q4"] }, q: { message: q.message } });
  }, 20_000);

  it("«Scarica un modello», then «Non ora»: the run ends without asking for a download", async () => {
    const api = new MockTransport();
    const { runId } = await start(api, "Scarica un modello");
    const q = (await waitFor(api, runId, custom("aiw.question"))).value as Question;
    await api.answerQuestion(q.id, { action: "decline" });
    expect((await waitFor(api, runId, custom("aiw.question_resolved"))).value).toEqual({ id: q.id, outcome: "declined", answer: null });
    await waitFor(api, runId, finished);
    expect(await api.approvals()).toEqual([]);
  }, 20_000);

  it("a web search runs before the question: it is what the warning on the later confirmation is about", async () => {
    const api = new MockTransport();
    const { runId } = await start(api, "Scarica un modello");
    await waitFor(api, runId, custom("aiw.question"));
    const names = (await api.runEvents(runId)).flatMap((e) => (e.type === "TOOL_CALL_START" ? [e.toolCallName] : []));
    expect(names).toEqual(["web_search", "ask_user"]);
    await api.cancelRun(runId);
  }, 20_000);

  it("multiple choice, confirmation and form questions are checked like the engine checks them", () => {
    const base: Question = { id: "a", run_id: "r", kind: "choice", title: null, message: "m", options: [{ id: "x", label: "X" }, { id: "y", label: "Y" }], multiple: true, allow_other: true, fields: [], confirm_label: null, deny_label: null };
    const answer = (a: Partial<{ selected: string[] | null; other: string | null; confirmed: boolean | null; values: Record<string, string | number | boolean> | null }>) =>
      ({ action: "answer" as const, selected: null, other: null, confirmed: null, values: null, ...a });
    expect(checkAnswer(base, answer({ selected: ["x", "y"], other: " altro " }))).toEqual({ selected: ["x", "y"], other: "altro", confirmed: null, values: null });
    expect(() => checkAnswer(base, answer({}))).toThrow(ApiError);
    expect(() => checkAnswer({ ...base, allow_other: false }, answer({ other: "z" }))).toThrow(/risposta libera/);
    expect(() => checkAnswer({ ...base, multiple: false }, answer({ selected: ["x", "y"] }))).toThrow(/una sola/);
    const confirm: Question = { ...base, kind: "confirm", options: [] };
    expect(checkAnswer(confirm, answer({ confirmed: false })).confirmed).toBe(false);
    expect(() => checkAnswer(confirm, answer({}))).toThrow(ApiError);
    const form: Question = { ...base, kind: "form", options: [], fields: [{ name: "n", label: "Nome", type: "string", required: true }, { name: "e", label: "Età", type: "integer" }, { name: "l", label: "Lingua", type: "enum", options: ["it", "en"] }] };
    expect(checkAnswer(form, answer({ values: { n: "Ada", e: 36, l: "en" } })).values).toEqual({ n: "Ada", e: 36, l: "en" });
    expect(() => checkAnswer(form, answer({ values: { e: 3 } }))).toThrow(/obbligatorio/);
    expect(() => checkAnswer(form, answer({ values: { n: "a", e: 3.5 } }))).toThrow(/tipo/);
    expect(() => checkAnswer(form, answer({ values: { n: "a", l: "fr" } }))).toThrow(/valori proposti/);
  });

  it("each of the other scripts asks its kind of question and ends after the answer", async () => {
    const api = new MockTransport();
    const kinds: [string, string][] = [["Quali modelli confrontiamo?", "choice"], ["Cancella i download vecchi", "confirm"], ["Crea un assistente per le email", "form"]];
    for (const [text, kind] of kinds) {
      const { runId } = await start(api, text);
      const q = (await waitFor(api, runId, custom("aiw.question"))).value as Question;
      expect(q.kind).toBe(kind);
      await api.answerQuestion(q.id, { action: "decline" });
      await waitFor(api, runId, finished);
    }
  }, 30_000);

  it("a question of a run that is cancelled stops waiting, and nothing is left open", async () => {
    const api = new MockTransport();
    const { runId } = await start(api, "Scarica un modello");
    await waitFor(api, runId, custom("aiw.question"));
    expect(await api.questions()).toHaveLength(1);
    await api.cancelRun(runId);
    await waitFor(api, runId, failed);
    expect(await api.questions()).toEqual([]);
    expect(await api.secretRequests()).toEqual([]);
  }, 20_000);

  it("the kill switch closes what waits too", async () => {
    const api = new MockTransport();
    const a = await start(api, "Scarica un modello");
    const b = await start(api, "Aggiungi il token di Hugging Face");
    await waitFor(api, a.runId, custom("aiw.question"));
    await waitFor(api, b.runId, custom("aiw.secret_request"));
    await api.kill();
    expect(await api.questions()).toEqual([]);
    expect(await api.secretRequests()).toEqual([]);
  }, 20_000);
});

describe("«Aggiungi il token di Hugging Face» and the secret", () => {
  it("asks for the secret with the events of the contract; the request never carries a value", async () => {
    const api = new MockTransport();
    const { runId } = await start(api, "Aggiungi il token di Hugging Face");
    const req = (await waitFor(api, runId, custom("aiw.secret_request"))).value as SecretRequest;
    expect(Object.keys(req).sort()).toEqual(["destination", "expires_at", "id", "label", "reason", "run_id", "target"]);
    expect(req).toMatchObject({ run_id: runId, target: "provider_token:hf", label: "Token di Hugging Face", destination: "Hugging Face (huggingface.co)" });
    expect(await api.secretRequests()).toEqual([req]);
    await api.answerSecretRequest(req.id, { action: "decline" });
    expect((await waitFor(api, runId, custom("aiw.secret_resolved"))).value).toEqual({ id: req.id, outcome: "declined" });
    await waitFor(api, runId, finished);
  }, 20_000);

  it("a refused value leaves the request open (and is not repeated in the error); a good one is stored; the value is nowhere afterwards", async () => {
    const api = new MockTransport();
    const { session, runId } = await start(api, "Aggiungi il token di Hugging Face");
    const req = (await waitFor(api, runId, custom("aiw.secret_request"))).value as SecretRequest;

    const WRONG = "not-a-hugging-face-token-xyz";
    const refused = await api.answerSecretRequest(req.id, { action: "provide", value: WRONG }).catch((e: unknown) => e);
    expect(refused).toBeInstanceOf(ApiError);
    expect((refused as ApiError).status).toBe(401);
    expect((refused as ApiError).message).not.toContain(WRONG);
    expect(await api.secretRequests()).toHaveLength(1);
    expect(reachableText([api, fixtures])).not.toContain(WRONG);

    await expect(api.answerSecretRequest(req.id, { action: "provide", value: "" })).rejects.toMatchObject({ status: 400 });

    expect(await api.answerSecretRequest(req.id, { action: "provide", value: FAKE_TOKEN })).toEqual({ stored: true });
    const resolved = await waitFor(api, runId, custom("aiw.secret_resolved"));
    expect(resolved.value).toEqual({ id: req.id, outcome: "stored" });
    await waitFor(api, runId, finished);
    await expect(api.answerSecretRequest(req.id, { action: "provide", value: FAKE_TOKEN })).rejects.toMatchObject({ status: 404 });

    // The provider is set up, the audit has the target…
    expect((await api.modelProviders()).find((p) => p.id === "hf")?.auth_configured).toBe(true);
    expect((await api.audit())[0]).toMatchObject({ action: "secret.set", detail: "provider_token:hf" });

    // …and the value is in no event, message, audit row, listing or state of the demo.
    const events = await api.runEvents(runId);
    const msgs = await api.messages(session.id);
    const seen = JSON.stringify([events, msgs, await api.audit(), await api.secretRequests(), await api.questions(), await api.approvals(), await api.modelProviders(), await api.sessions()]);
    expect(seen).not.toContain(FAKE_TOKEN);
    expect(seen).not.toContain("hf_TEST_ONLY");
    expect(reachableText([api, fixtures])).not.toContain(FAKE_TOKEN);

    // The stored conversation: what was asked and how it ended, only.
    const result = msgs.find((m) => m.role === "tool");
    expect(JSON.parse(result?.content ?? "{}")).toEqual({ outcome: "stored", target: "provider_token:hf" });
    const row = historyToItems(msgs).find((i): i is SecretItem => i.kind === "secret");
    expect(row).toMatchObject({ outcome: "stored", req: { target: "provider_token:hf", label: "Token di Hugging Face" } });
    const live = events.reduce((items, e) => reduce(items, e), [] as ReturnType<typeof historyToItems>).find((i): i is SecretItem => i.kind === "secret");
    expect(live).toMatchObject({ outcome: "stored", req: { target: "provider_token:hf" } });
  }, 20_000);
});
