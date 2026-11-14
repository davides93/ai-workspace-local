import { ApiError } from "../http";
import type { AgEvent, ChatMessage, Question, QuestionAnswer, QuestionOutcome, QuestionReply, SecretOutcome, SecretReply, SecretRequest } from "../types";

// The script of the Assistente in the demo: it asks the person (`ask_user`) and asks for a secret (`aiw_secret_request`) like
// the engine will, with the same events and the same results for the model (docs of the harness, C.2 and C.3). The run waits for
// the answer, then goes on. The value of a secret is never kept: it is checked, used to mark the provider as set up, and
// dropped; it is not in a message, an event, an audit row or any state of the demo.

export type AssistantScript = "download" | "multiple" | "confirm" | "form" | "secret";

/** The phrases that start each script (any session, whatever the agent), compared without case and accents. */
const PHRASES: [AssistantScript, string][] = [
  ["download", "scarica un modello"],
  ["multiple", "quali modelli confrontiamo"],
  ["confirm", "cancella i download vecchi"],
  ["form", "crea un assistente per le email"],
  ["secret", "aggiungi il token di hugging face"],
];

const plain = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

export function assistantScript(input: string): AssistantScript | null {
  const text = plain(input);
  return PHRASES.find(([, phrase]) => text.includes(phrase))?.[0] ?? null;
}

/** How long the engine waits for an answer, like `approvals.timeout_s` (300 s by default): only the date in the event; the demo does not expire. */
const WAIT_S = 300;

interface Waiting<R> {
  runId: string;
  resolve: (r: R) => void;
}
export interface QuestionResult { outcome: QuestionOutcome; answer: QuestionAnswer | null }

const bad = (message: string) => new ApiError(400, message);
const gone = (what: string) => new ApiError(404, `${what} non trovata (forse già chiusa)`);

/**
 * The checks the engine makes on an answer (`POST /api/questions/{id}`): ids that exist, one pick for a single answer, «Altro»
 * only when it is allowed, a boolean for a confirmation, the values of a form against its fields. A wrong answer is a 400 and
 * the question stays open.
 */
export function checkAnswer(q: Question, r: Extract<QuestionReply, { action: "answer" }>): QuestionAnswer {
  if (q.kind === "confirm") {
    if (typeof r.confirmed !== "boolean") throw bad("la conferma richiede «sì» o «no»");
    return { selected: null, other: null, confirmed: r.confirmed, values: null };
  }
  if (q.kind === "form") {
    const values = r.values ?? {};
    const out: Record<string, string | number | boolean> = {};
    for (const f of q.fields) {
      const v = values[f.name];
      if (v === undefined || v === "") {
        if (f.required) throw bad(`il campo «${f.label}» è obbligatorio`);
        continue;
      }
      if (f.type === "boolean" ? typeof v !== "boolean" : f.type === "number" || f.type === "integer" ? typeof v !== "number" || !Number.isFinite(v) || (f.type === "integer" && !Number.isInteger(v)) : typeof v !== "string") {
        throw bad(`il campo «${f.label}» non ha il tipo giusto`);
      }
      if (f.type === "enum" && !(f.options ?? []).includes(v as string)) throw bad(`il campo «${f.label}» deve essere uno dei valori proposti`);
      out[f.name] = v;
    }
    return { selected: null, other: null, confirmed: null, values: out };
  }
  const selected = r.selected ?? [];
  if (selected.some((id) => !q.options.some((o) => o.id === id))) throw bad("la risposta nomina un'opzione che non esiste");
  if (!q.multiple && selected.length > 1) throw bad("la domanda ammette una sola risposta");
  const other = r.other?.trim() ? r.other.trim() : null;
  if (other !== null && !q.allow_other) throw bad("la domanda non ammette una risposta libera");
  if (selected.length === 0 && other === null) throw bad("scegli almeno una risposta");
  return { selected: selected.length > 0 ? selected : null, other, confirmed: null, values: null };
}

/**
 * The questions and the requests for a secret that wait for the person, in memory like the engine keeps them. It holds no value:
 * a secret is handed to `answerSecret`, checked, and gone.
 */
export class AskStore {
  private questions = new Map<string, { q: Question } & Waiting<QuestionResult>>();
  private secrets = new Map<string, { req: SecretRequest } & Waiting<SecretOutcome>>();

  openQuestions(): Question[] { return [...this.questions.values()].map((w) => structuredClone(w.q)); }
  openSecrets(): SecretRequest[] { return [...this.secrets.values()].map((w) => structuredClone(w.req)); }

  /** Registers the question and waits until it is answered, declined or its run is cancelled. */
  waitQuestion(q: Question): Promise<QuestionResult> {
    return new Promise((resolve) => this.questions.set(q.id, { q, runId: q.run_id, resolve }));
  }
  waitSecret(req: SecretRequest): Promise<SecretOutcome> {
    return new Promise((resolve) => this.secrets.set(req.id, { req, runId: req.run_id, resolve }));
  }

  answerQuestion(id: string, reply: QuestionReply): void {
    const w = this.questions.get(id);
    if (!w) throw gone("domanda");
    const result: QuestionResult = reply.action === "decline" ? { outcome: "declined", answer: null } : { outcome: "answered", answer: checkAnswer(w.q, reply) };
    this.questions.delete(id);
    w.resolve(result);
  }

  /**
   * `provide(target, value)` writes the secret where it belongs and throws when the destination refuses it (the request stays
   * open, the person can try again). The value is not kept here.
   */
  answerSecret(id: string, reply: SecretReply, provide: (target: string, value: string) => void): { stored: boolean } {
    const w = this.secrets.get(id);
    if (!w) throw gone("richiesta");
    if (reply.action === "decline") {
      this.secrets.delete(id);
      w.resolve("declined");
      return { stored: false };
    }
    if (typeof reply.value !== "string" || reply.value === "") throw bad("manca il valore");
    provide(w.req.target, reply.value);
    this.secrets.delete(id);
    w.resolve("stored");
    return { stored: true };
  }

  /** The run was cancelled or killed: nobody waits for an answer any more. */
  cancelRun(runId?: string): void {
    for (const [id, w] of this.questions) {
      if (runId !== undefined && w.runId !== runId) continue;
      this.questions.delete(id);
      w.resolve({ outcome: "cancelled", answer: null });
    }
    for (const [id, w] of this.secrets) {
      if (runId !== undefined && w.runId !== runId) continue;
      this.secrets.delete(id);
      w.resolve("cancelled");
    }
  }
}

/** The destination a token goes to: what the engine would write, and what it checks before (the demo knows Hugging Face only). */
export function checkProviderToken(target: string, value: string): void {
  if (target !== "provider_token:hf") return;
  // Hugging Face tokens begin with `hf_`. The message never repeats the value.
  if (!value.startsWith("hf_") || value.length < 12) throw new ApiError(401, "Hugging Face ha rifiutato il token: non sembra valido (un token di Hugging Face inizia con «hf_»).");
}

// ---------------------------------------------------------------- the scripts

/** What the script needs from the demo transport. */
export interface AssistantDriver {
  runId: string;
  threadId: string;
  ask: AskStore;
  emit(e: AgEvent): void;
  nid(prefix: string): string;
  /** Streams an assistant message (start, content, end). */
  say(text: string): Promise<void>;
  /** A tool call that needs no confirmation or asks for one; `id` is the id of the call. Resolves to whether it ran. */
  tool(name: string, args: unknown, result: string, id: string, approval?: { reason: string; risk: "low" | "medium" | "high" }): Promise<boolean>;
  /** Ends the run like the others: the last message, stored with the rest. */
  finish(stored: ChatMessage[], final: string): void;
  now(): string;
}

const MODELS = [
  { id: "q4", label: "Q4_K_M · 4,7 GB", description: "Consigliata" },
  { id: "q5", label: "Q5_K_M · 5,6 GB", description: "Un po' più precisa" },
  { id: "q8", label: "Q8_0 · 8,5 GB", description: null },
];

const expires = () => new Date(Date.now() + WAIT_S * 1000).toISOString();

/** Stores a tool call with its result like the engine does: the assistant message that holds the call (with the words before it), then the result. */
type RecordCall = (callId: string, name: string, args: unknown, result: string) => void;

/** `ask_user`: the call, the question event, the wait, the closing event and the result for the model. `null` when the run was cancelled. */
async function askUser(d: AssistantDriver, record: RecordCall, args: { [k: string]: unknown }): Promise<QuestionResult | null> {
  const callId = d.nid("call");
  const q: Question = {
    id: d.nid("ask"), run_id: d.runId, kind: args.kind as Question["kind"], title: (args.title as string | undefined) ?? null, message: args.message as string,
    options: (args.options as Question["options"] | undefined)?.map((o) => ({ description: null, ...o })) ?? [], multiple: args.multiple === true, allow_other: args.allow_other === true,
    fields: (args.fields as Question["fields"] | undefined) ?? [], confirm_label: (args.confirm_label as string | undefined) ?? null, deny_label: (args.deny_label as string | undefined) ?? null,
    expires_at: expires(),
  };
  d.emit({ type: "TOOL_CALL_START", toolCallId: callId, toolCallName: "ask_user" });
  d.emit({ type: "TOOL_CALL_ARGS", toolCallId: callId, delta: JSON.stringify(args) });
  d.emit({ type: "TOOL_CALL_END", toolCallId: callId });
  const waiting = d.ask.waitQuestion(q);
  d.emit({ type: "CUSTOM", name: "aiw.question", value: q });
  const r = await waiting;
  d.emit({ type: "CUSTOM", name: "aiw.question_resolved", value: { id: q.id, outcome: r.outcome, answer: r.answer } });
  if (r.outcome === "cancelled") return null;
  const content = JSON.stringify({ outcome: r.outcome, ...(r.answer ?? {}) });
  d.emit({ type: "TOOL_CALL_RESULT", messageId: d.nid("m"), toolCallId: callId, content });
  record(callId, "ask_user", args, content);
  return r;
}

const labelsOf = (q: { options: { id: string; label: string }[] }, ids: string[] | null) => (ids ?? []).map((id) => q.options.find((o) => o.id === id)?.label ?? id);

/** Runs one of the scripts. The run is started and ended here; `finish` stores the messages and closes the session's status. */
export async function runAssistantScript(d: AssistantDriver, script: AssistantScript): Promise<void> {
  d.emit({ type: "RUN_STARTED", threadId: d.threadId, runId: d.runId });
  const stored: ChatMessage[] = [];
  // The words the assistant said just before a call go in the same stored message as the call.
  let lead = "";
  const say = async (text: string) => {
    lead = text;
    await d.say(text);
  };
  const record: RecordCall = (callId, name, args, result) => {
    stored.push(
      { id: d.nid("m"), role: "assistant", content: lead, tool_calls: [{ id: callId, name, arguments: JSON.stringify(args) }], created_at: d.now() },
      { id: d.nid("m"), role: "tool", content: result, tool_call_id: callId, created_at: d.now() },
    );
    lead = "";
  };
  const end = async (final: string) => {
    await d.say(final);
    d.emit({ type: "RUN_FINISHED", threadId: d.threadId, runId: d.runId });
    d.finish(stored, final);
  };

  if (script === "download") {
    await say("Cerco le varianti adatte a questo computer.");
    const searchId = d.nid("call");
    const search = { query: "Qwen3 4B GGUF varianti quantizzazione 16 GB RAM" };
    const found = "3 risultati: Qwen3-4B-GGUF (Q4_K_M 4,7 GB · Q5_K_M 5,6 GB · Q8_0 8,5 GB).";
    await d.tool("web_search", search, found, searchId);
    record(searchId, "web_search", search, found);
    const args = { kind: "choice", title: "Quale modello?", message: "Ho trovato tre varianti adatte a questo computer.", options: MODELS.map((o) => (o.description ? o : { id: o.id, label: o.label })) };
    const r = await askUser(d, record, args);
    if (!r) return;
    if (r.outcome !== "answered") return end("Va bene, non scarico nulla.");
    const pick = MODELS.find((o) => o.id === r.answer?.selected?.[0]) ?? MODELS[0];
    const [variant, size] = pick.label.split(" · ");
    await say(`Scarico la variante ${variant}. Per questa azione serve la tua conferma.`);
    const dlId = d.nid("call");
    const dl = { model: "Qwen/Qwen3-4B-GGUF", variant: pick.id };
    const started = `Download avviato: Qwen3-4B ${variant} (${size}).`;
    const ok = await d.tool("aiw_model_download", dl, started, dlId, { reason: `Scarica il modello Qwen3-4B (${pick.label}) nella cartella dei modelli.`, risk: "medium" });
    record(dlId, "aiw_model_download", dl, ok ? started : "Negato dall'utente.");
    return end(ok ? "Fatto: il download è partito, lo trovi in Modelli." : "Ok, non scarico nulla.");
  }

  if (script === "multiple") {
    await say("Ti propongo qualche modello da mettere a confronto.");
    const args = {
      kind: "choice", title: "Quali modelli?", message: "Scegli i modelli da confrontare (puoi sceglierne più di uno).", multiple: true, allow_other: true,
      options: [{ id: "qwen3", label: "Qwen3 4B" }, { id: "gemma", label: "Gemma 4 E4B" }, { id: "llama", label: "Llama 3.2 3B" }],
    };
    const r = await askUser(d, record, args);
    if (!r) return;
    if (r.outcome !== "answered") return end("Va bene, rimandiamo il confronto.");
    const picks = [...labelsOf(args, r.answer?.selected ?? null), ...(r.answer?.other ? [r.answer.other] : [])];
    return end(`Confronto ${picks.join(", ")}: preparo una prova identica per ognuno.`);
  }

  if (script === "confirm") {
    await say("Guardo quali download sono vecchi.");
    const args = { kind: "confirm", title: "Cancello i vecchi?", message: "Ci sono 3 download interrotti che occupano 2,1 GB. Li cancello?", confirm_label: "Cancella", deny_label: "Tienili" };
    const r = await askUser(d, record, args);
    if (!r) return;
    if (r.outcome !== "answered") return end("Va bene, ne riparliamo un'altra volta.");
    return end(r.answer?.confirmed ? "Fatto: ho cancellato i 3 download interrotti (2,1 GB liberati)." : "Va bene, li tengo.");
  }

  if (script === "form") {
    await say("Mi servono due dati per prepararlo.");
    const args = {
      kind: "form", title: "Il nuovo assistente", message: "Come lo vuoi?",
      fields: [
        { name: "nome", label: "Nome", type: "string", required: true, placeholder: "Posta" },
        { name: "lingua", label: "Lingua", type: "enum", options: ["italiano", "inglese"], required: true },
        { name: "bozze", label: "Quante bozze alla volta", type: "integer" },
        { name: "da_solo", label: "Risponde da solo", type: "boolean" },
      ],
    };
    const r = await askUser(d, record, args);
    if (!r) return;
    if (r.outcome !== "answered") return end("Va bene, non creo nulla.");
    const v = r.answer?.values ?? {};
    return end(`Preparo «${String(v.nome ?? "Posta")}» in ${String(v.lingua ?? "italiano")}${v.da_solo ? ", con risposta automatica" : ", con le bozze da approvare"}.`);
  }

  // script === "secret"
  await say("Mi serve il tuo token di Hugging Face: lo inserisci tu nel campo qui sotto, io non lo vedo.");
  const callId = d.nid("call");
  const args = { target: "provider_token:hf", label: "Token di Hugging Face", reason: "Serve per scaricare modelli riservati" };
  const req: SecretRequest = { id: d.nid("sec"), run_id: d.runId, ...args, destination: "Hugging Face (huggingface.co)", expires_at: expires() };
  d.emit({ type: "TOOL_CALL_START", toolCallId: callId, toolCallName: "aiw_secret_request" });
  d.emit({ type: "TOOL_CALL_ARGS", toolCallId: callId, delta: JSON.stringify(args) });
  d.emit({ type: "TOOL_CALL_END", toolCallId: callId });
  const waiting = d.ask.waitSecret(req);
  d.emit({ type: "CUSTOM", name: "aiw.secret_request", value: req });
  const outcome = await waiting;
  d.emit({ type: "CUSTOM", name: "aiw.secret_resolved", value: { id: req.id, outcome } });
  if (outcome === "cancelled") return;
  const content = JSON.stringify({ outcome, ...(outcome === "stored" ? { target: args.target } : {}) });
  d.emit({ type: "TOOL_CALL_RESULT", messageId: d.nid("m"), toolCallId: callId, content });
  record(callId, "aiw_secret_request", args, content);
  return end(outcome === "stored" ? "Il token è salvato: ora posso scaricare anche i modelli riservati." : "Va bene, non aggiungo il token.");
}
