import { ApiError } from "../http";
import { MAX_TASK_INSTRUCTION_CHARS } from "../types";
import type { ApprovalRequest, BackgroundTask, BudgetLimits, TaskCreate, TaskPauseReason, TaskQuery, TaskResume, TaskStatus } from "../types";
import { nid } from "./fixtures";

// Demo of the background tasks API: the contract of the engine (statuses, pauses, the 409 sentences) as a small state machine.
//
// Nothing here moves by itself. A pause that waits for the end of the step, an instruction read at the next step and the
// confirmation a resumed task asks for happen on the first read after their time (`settle`), so a demo that nobody looks at
// stays still and the tests can drive the clock (`vi.setSystemTime`). Every `MockTransport` has its own store.

const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

/** How long finishing the current step takes after a pause was asked. */
export const STEP_MS = 1500;
/** How long a resumed task takes to read what was said while it was paused. */
export const READ_MS = 1000;
/** How long a resumed task takes to ask for its first confirmation. */
export const ASK_MS = 1500;

export const TASK_STATUSES: readonly TaskStatus[] = ["queued", "running", "paused", "succeeded", "failed", "cancelled"];

/** Pauses that come from a rule of the proactive assistant: the person can force them. */
const RULE_REASONS = ["quiet_hours", "battery", "daily_budget"] as const;
type RuleReason = (typeof RULE_REASONS)[number];
const isRule = (r: TaskPauseReason | undefined): r is RuleReason => !!r && (RULE_REASONS as readonly string[]).includes(r);

/** What each limit counts, to add the extension to what the task has used when it had no limit of that kind. */

/** `?scenario=first-run`: a new installation has no task. */
export const taskScenario = { empty: false };

const finished = (t: BackgroundTask) => t.status === "succeeded" || t.status === "failed" || t.status === "cancelled";

const STATUS_WORDS: Record<TaskStatus, string> = {
  queued: "in coda", running: "in corso", paused: "in pausa", succeeded: "finito", failed: "non riuscito", cancelled: "annullato",
};

const RULE_REFUSAL: Record<RuleReason, string> = {
  quiet_hours: "È ancora l'orario di silenzio (fino alle 07:00): il lavoro ripartirà da solo alla fine. Se vuoi che continui adesso usa «Riprendi comunque».",
  battery: "Il computer è a batteria e le regole dell'assistente chiedono di risparmiarla: il lavoro ripartirà da solo quando lo colleghi alla corrente. Se vuoi che continui adesso usa «Riprendi comunque».",
  daily_budget: "Il tempo di oggi per i lavori in background è finito: il lavoro ripartirà da solo domani. Se vuoi che continui adesso usa «Riprendi comunque».",
};

/** Something the task still has to ask for after a resume (the demo's scripted confirmations). */
type Ask = (task: BackgroundTask) => ApprovalRequest;

interface Timers {
  stopAt?: number;
  readAt?: number;
  askAt?: number;
}

const base = (over: Partial<BackgroundTask> & Pick<BackgroundTask, "id" | "title" | "instruction" | "session_id" | "status">): BackgroundTask => ({
  agent_id: "operator", origin: "user", auto_resume: false, stopping: false, forced: false, segments: 1, run_ids: [], limits: {},
  progress: { turns: 0, tokens: 0, seconds: 0 }, instructions: [], notes: [], created_at: ago(60), updated_at: ago(5),
  ...over,
});

/** The demo workspace: one task at work, three paused for different reasons, two finished. */
function seed(): { tasks: BackgroundTask[]; script: Map<string, Ask[]> } {
  const run = base({
    id: "tsk_demo_run", title: "Archiviare le fatture di ottobre", session_id: "ses_pdf", status: "running", segments: 2,
    instruction: "Guarda i file nuovi in Scaricati e sposta le fatture di ottobre in Documenti/Fatture, una cartella per fornitore.",
    run_ids: ["run_tsk_demo_a", "run_tsk_demo_b"], current_run_id: "run_tsk_demo_b", created_at: ago(35), updated_at: ago(1),
    limits: { max_seconds: 1800, max_turns: 40 },
    progress: { turns: 3, tokens: 5200, seconds: 240, last_tool: "list_dir", last_text: "Ho trovato 4 file che sembrano fatture: due di Rossi Srl e due di Bianchi SpA. Sto preparando le cartelle.", updated_at: ago(1) },
    instructions: [{ id: "tin_demo_1", text: "Metti le fatture di Rossi Srl in una cartella a parte.", source: "user", created_at: ago(12), status: "delivered", delivered_at: ago(10) }],
    notes: [
      { at: ago(35), text: "Avviato." },
      { at: ago(20), text: "In pausa fino al tuo ritorno." },
      { at: ago(15), text: "Ripreso." },
      { at: ago(10), text: "Ha letto la tua istruzione: «Metti le fatture di Rossi Srl in una cartella a parte.»" },
    ],
  });
  const quiet = base({
    id: "tsk_demo_quiet", title: "Confrontare le offerte dei fornitori", session_id: "ses_cv", agent_id: "browser", status: "paused", segments: 1,
    instruction: "Confronta le tre offerte ricevute questa settimana e preparami una tabella con prezzi e tempi di consegna.",
    run_ids: ["run_tsk_demo_c"], created_at: ago(180), updated_at: ago(30),
    pause_reason: "quiet_hours", pause_text: "È il tuo orario di silenzio (fino alle 07:00): riparte da solo quando finisce.", auto_resume: true,
    limits: { max_seconds: 1800 },
    progress: { turns: 5, tokens: 9100, seconds: 540, last_tool: "web_fetch", last_text: "Ho letto le prime due offerte: manca solo quella di Bianchi SpA.", updated_at: ago(30) },
    notes: [{ at: ago(180), text: "Avviato." }, { at: ago(30), text: "In pausa: orario di silenzio." }],
  });
  const budget = base({
    id: "tsk_demo_budget", title: "Prenotare la sala riunioni per giovedì", session_id: "ses_ui", status: "paused", segments: 1,
    instruction: "Prenota la sala grande per giovedì mattina e avvisa i colleghi con un invito.",
    run_ids: ["run_tsk_demo_d"], created_at: ago(240), updated_at: ago(70),
    pause_reason: "task_budget", pause_text: "Ha usato tutto il tempo che gli avevi dato (30 minuti): per farlo continuare aggiungi tempo.",
    limits: { max_seconds: 1800 },
    progress: { turns: 11, tokens: 21400, seconds: 1800, last_tool: "browser_click", last_text: "La sala grande è libera giovedì dalle 9: sto compilando il modulo di prenotazione.", updated_at: ago(70) },
    instructions: [{ id: "tin_demo_2", text: "Cancella anche la prenotazione di venerdì.", source: "user", created_at: ago(75), status: "blocked", note: "Non l'ho eseguita: cancellare una prenotazione esistente non rientra in questo lavoro." }],
    notes: [{ at: ago(240), text: "Avviato." }, { at: ago(75), text: "Istruzione non eseguita: «Cancella anche la prenotazione di venerdì.»" }, { at: ago(70), text: "In pausa: ha finito il suo budget." }],
  });
  const restart = base({
    id: "tsk_demo_restart", title: "Sistemare il test del parser GGUF", session_id: "ses_bug", agent_id: "coder", status: "paused", segments: 1,
    instruction: "Trova perché il test del parser GGUF fallisce e proponi una correzione.",
    run_ids: ["run_tsk_demo_e"], created_at: ago(500), updated_at: ago(120),
    pause_reason: "restart", pause_text: "Si è interrotto perché l'app è stata riavviata: non riparte da solo, riprendilo quando vuoi.",
    progress: { turns: 7, tokens: 15800, seconds: 760, last_tool: "read_file", last_text: "Il problema sembra nella lettura dell'intestazione: guardo come vengono contati i tensori.", updated_at: ago(122) },
    notes: [{ at: ago(500), text: "Avviato." }, { at: ago(120), text: "Interrotto dal riavvio dell'app." }],
  });
  const done = base({
    id: "tsk_demo_done", title: "Riassumere le note della riunione", session_id: "ses_pdf", origin: "proposal", proposal_id: "prp_demo_done", status: "succeeded",
    instruction: "Riassumi le note della riunione in cinque punti.", run_ids: ["run_demo_done"], created_at: ago(1500), updated_at: ago(1400), finished_at: ago(1400),
    progress: { turns: 4, tokens: 6100, seconds: 190, last_tool: "write_file", last_text: "Fatto.", updated_at: ago(1400) },
    result: "Ho riassunto le note in cinque punti e le ho salvate in Documenti/Riunioni/riassunto.md.",
    notes: [{ at: ago(1500), text: "Avviato dalla proposta." }, { at: ago(1400), text: "Finito." }],
  });
  const failed = base({
    id: "tsk_demo_failed", title: "Archiviare le email vecchie", session_id: "ses_cv", agent_id: "browser", status: "failed",
    instruction: "Sposta in archivio le email di più di un anno fa.", run_ids: ["run_tsk_demo_f"], created_at: ago(3000), updated_at: ago(2900), finished_at: ago(2900),
    progress: { turns: 1, tokens: 800, seconds: 20, last_tool: "mail_search", updated_at: ago(2900) },
    error: "Non riesco a leggere la posta: la password dell'account non è più valida.",
    notes: [{ at: ago(3000), text: "Avviato." }, { at: ago(2900), text: "Non è riuscito: la posta non risponde." }],
  });

  // What the first task asks for once it is resumed: the first confirmation, then a second one (a person who allows one and denies the other).
  const script = new Map<string, Ask[]>();
  script.set(run.id, [
    (t) => ({ id: nid("apv"), run_id: t.current_run_id ?? "", tool: "shell", args: { command: "mv ~/Scaricati/fattura-rossi-*.pdf ~/Documenti/Fatture/Rossi" }, reason: "Il comando sposta 2 file dalla cartella Scaricati a Documenti/Fatture/Rossi.", risk: "medium" }),
    (t) => ({ id: nid("apv"), run_id: t.current_run_id ?? "", tool: "write_file", args: { path: "~/Documenti/Fatture/indice.txt" }, reason: "Scrive in un file nuovo l'elenco delle fatture riordinate.", risk: "low" }),
  ]);
  return { tasks: [run, quiet, budget, restart, done, failed], script };
}

const positive = (n: unknown): n is number => typeof n === "number" && Number.isInteger(n) && n >= 1;

/** The limits to add (`extend`) or to start with: each one a whole number of at least 1. */
function checkLimits(limits: BudgetLimits | undefined, what: string): BudgetLimits {
  const out: BudgetLimits = {};
  for (const key of ["max_tokens", "max_turns", "max_seconds"] as const) {
    const v = limits?.[key];
    if (v === undefined || v === null) continue;
    if (!positive(v)) throw new ApiError(400, `${what}: «${key}» deve essere un numero intero di almeno 1`);
    out[key] = v;
  }
  return out;
}

export class TaskStore {
  readonly tasks: BackgroundTask[];
  /**
   * Which rules of the proactive assistant are in force now. The demo is in quiet hours, on battery and out of the day's time,
   * so the tasks paused for those reasons wait for them. `quietNow` puts every other pause (the person's, the restart's) in
   * quiet hours too, so a plain resume of any task is refused.
   */
  readonly rules = { quiet_hours: true, battery: true, daily_budget: true, quietNow: false };
  private readonly timers = new Map<string, Timers>();
  private readonly script: Map<string, Ask[]>;

  constructor(withDemoData: boolean = !taskScenario.empty) {
    const s = withDemoData ? seed() : { tasks: [], script: new Map<string, Ask[]>() };
    this.tasks = s.tasks;
    this.script = s.script;
  }

  private find(id: string): BackgroundTask {
    const t = this.tasks.find((x) => x.id === id);
    if (!t) throw new ApiError(404, `lavoro ${id} non trovato`);
    return t;
  }

  private touch(t: BackgroundTask, note?: string): void {
    const at = new Date().toISOString();
    t.updated_at = at;
    if (note) t.notes.push({ at, text: note });
  }

  private timer(id: string): Timers {
    let tm = this.timers.get(id);
    if (!tm) this.timers.set(id, (tm = {}));
    return tm;
  }

  private enterPause(t: BackgroundTask, reason: TaskPauseReason, text: string, note: string, autoResume = false): void {
    // A confirmation it was waiting for is asked again when it resumes.
    if (t.waiting_approval) {
      const req = t.waiting_approval;
      this.script.set(t.id, [() => req, ...(this.script.get(t.id) ?? [])]);
      t.waiting_approval = undefined;
    }
    Object.assign(t, { status: "paused", pause_reason: reason, pause_text: text, auto_resume: autoResume, stopping: false, forced: false });
    const tm = this.timer(t.id);
    tm.stopAt = tm.readAt = tm.askAt = undefined;
    this.touch(t, note);
  }

  /** Applies what became due: a pause that waited for the end of the step, the instructions read, the next confirmation. */
  private settle(): void {
    const at = Date.now();
    for (const t of this.tasks) {
      const tm = this.timers.get(t.id);
      if (!tm) continue;
      if (tm.stopAt !== undefined && tm.stopAt <= at && t.status === "running" && t.stopping) {
        t.progress.turns += 1;
        this.enterPause(t, "user", "L'hai messo in pausa tu: si è fermato dopo l'ultimo passo.", "In pausa, come avevi chiesto.");
      }
      if (tm.readAt !== undefined && tm.readAt <= at && t.status === "running" && !t.stopping) {
        tm.readAt = undefined;
        for (const i of t.instructions.filter((x) => x.status === "pending")) {
          i.status = "delivered";
          i.delivered_at = new Date(at).toISOString();
          t.progress.turns += 1;
          this.touch(t, `Ha letto la tua istruzione: «${i.text.length > 80 ? `${i.text.slice(0, 80)}…` : i.text}»`);
        }
      }
      if (tm.askAt !== undefined && tm.askAt <= at && t.status === "running" && !t.stopping && !t.waiting_approval) {
        tm.askAt = undefined;
        const next = this.script.get(t.id)?.shift();
        if (next) {
          t.waiting_approval = next(t);
          this.touch(t);
        }
      }
    }
  }

  list(query: TaskQuery = {}): BackgroundTask[] {
    if (query.status && !TASK_STATUSES.includes(query.status)) throw new ApiError(400, `stato non valido «${query.status}»: usa ${TASK_STATUSES.join(", ")}`);
    this.settle();
    return structuredClone(this.tasks.filter((t) => !query.status || t.status === query.status).sort((a, b) => b.created_at.localeCompare(a.created_at)));
  }

  get(id: string): BackgroundTask {
    this.settle();
    return structuredClone(this.find(id));
  }

  /** What `POST /api/tasks` refuses, checked before anything (a session) is made for it. */
  static check(body: TaskCreate): { instruction: string; title: string; limits: BudgetLimits } {
    const instruction = body.instruction?.trim() ?? "";
    if (!instruction) throw new ApiError(400, "scrivi cosa deve fare il lavoro");
    return { instruction, title: body.title?.trim() || instruction.split("\n")[0].slice(0, 60), limits: checkLimits(body.limits, "limiti") };
  }

  create(body: TaskCreate, sessionId: string): BackgroundTask {
    const { instruction, title, limits } = TaskStore.check(body);
    const runId = nid("run");
    const at = new Date().toISOString();
    const t = base({
      id: nid("tsk"), title, instruction, session_id: sessionId, agent_id: body.agent_id ?? "operator",
      status: "running", run_ids: [runId], current_run_id: runId, limits, created_at: at, updated_at: at, notes: [{ at, text: "Avviato da te." }],
    });
    this.tasks.unshift(t);
    return structuredClone(t);
  }

  pause(id: string): BackgroundTask {
    this.settle();
    const t = this.find(id);
    if (finished(t)) throw new ApiError(409, `il lavoro «${t.title}» è già finito: non c'è niente da mettere in pausa`);
    if (t.status === "paused") throw new ApiError(409, `il lavoro «${t.title}» è già in pausa`);
    if (t.status === "queued" || t.waiting_approval) {
      // Waiting (for its turn or for a confirmation) there is no step to finish.
      this.enterPause(t, "user", "L'hai messo in pausa tu.", "In pausa, come avevi chiesto.");
    } else if (!t.stopping) {
      t.stopping = true;
      this.timer(t.id).stopAt = Date.now() + STEP_MS;
      this.touch(t, "Pausa chiesta: si ferma quando finisce il passo.");
    }
    return structuredClone(t);
  }

  /** The rule of the proactive assistant that keeps this task from starting now, if any. */
  private ruleInForce(t: BackgroundTask): RuleReason | undefined {
    const r = t.pause_reason;
    if (isRule(r)) return this.rules[r] ? r : undefined;
    return this.rules.quietNow ? "quiet_hours" : undefined;
  }

  resume(id: string, body: TaskResume = {}): BackgroundTask {
    this.settle();
    const t = this.find(id);
    const extend = checkLimits(body.extend, "estensione");
    if (t.status !== "paused" && t.status !== "queued") throw new ApiError(409, `il lavoro «${t.title}» non è in pausa (è ${STATUS_WORDS[t.status]})`);
    if (t.pause_reason === "kill_switch") throw new ApiError(409, "Il lavoro è stato fermato dal pulsante di emergenza: riaccendi l'assistente dalle impostazioni e poi riprendilo. Nemmeno «Riprendi comunque» lo fa ripartire.");
    const extended = Object.keys(extend).length > 0;
    if (t.pause_reason === "task_budget" && !extended) throw new ApiError(409, "Il lavoro ha finito il suo budget: aggiungi un po' di tempo (per esempio 10 minuti) per riprenderlo.");
    const rule = this.ruleInForce(t);
    if (rule && !body.force) throw new ApiError(409, RULE_REFUSAL[rule]);
    for (const key of ["max_tokens", "max_turns", "max_seconds"] as const) {
      const add = extend[key];
      // Only the limits the task has are raised (a task without a token limit does not get one), as the engine does.
      const current = t.limits[key];
      if (add !== undefined && current !== undefined) t.limits[key] = current + add;
    }
    const runId = nid("run");
    const forced = !!body.force && !!rule;
    Object.assign(t, {
      status: "running", pause_reason: undefined, pause_text: undefined, auto_resume: false, stopping: false, forced,
      segments: t.segments + 1, current_run_id: runId, run_ids: [...t.run_ids, runId],
    });
    const tm = this.timer(t.id);
    if (t.instructions.some((i) => i.status === "pending")) tm.readAt = Date.now() + READ_MS;
    if (this.script.get(t.id)?.length) tm.askAt = Date.now() + ASK_MS;
    this.touch(t, forced ? "Ripreso comunque, passando sopra la regola." : extended ? "Ripreso con un po' di budget in più." : "Ripreso.");
    return structuredClone(t);
  }

  instruct(id: string, text: string, source: "user" | "voice" = "user"): BackgroundTask {
    this.settle();
    const t = this.find(id);
    const clean = text?.trim() ?? "";
    if (!clean) throw new ApiError(400, "scrivi cosa vuoi dire al lavoro");
    if ([...clean].length > MAX_TASK_INSTRUCTION_CHARS) throw new ApiError(400, `l'istruzione supera i ${MAX_TASK_INSTRUCTION_CHARS} caratteri`);
    if (source !== "user" && source !== "voice") throw new ApiError(400, `origine non valida «${String(source)}»: usa user o voice`);
    if (finished(t)) throw new ApiError(409, `il lavoro «${t.title}» è già finito: non può più leggere istruzioni`);
    t.instructions.push({ id: nid("tin"), text: clean, source, created_at: new Date().toISOString(), status: "pending" });
    // At work it reads it at the next step; paused, it keeps it for when it starts again.
    if (t.status === "running" && !t.stopping) this.timer(t.id).readAt = Date.now() + READ_MS;
    this.touch(t);
    return structuredClone(t);
  }

  cancel(id: string): BackgroundTask {
    this.settle();
    const t = this.find(id);
    if (finished(t)) throw new ApiError(409, `il lavoro «${t.title}» è già finito (${STATUS_WORDS[t.status]})`);
    const at = new Date().toISOString();
    for (const i of t.instructions.filter((x) => x.status === "pending")) Object.assign(i, { status: "blocked", note: "Il lavoro è stato annullato prima di leggerla." });
    Object.assign(t, { status: "cancelled", stopping: false, waiting_approval: undefined, pause_reason: undefined, pause_text: undefined, auto_resume: false, finished_at: at });
    this.timers.delete(t.id);
    this.script.delete(t.id);
    this.touch(t, "Annullato da te.");
    return structuredClone(t);
  }

  /** The emergency button: whatever is at work stops, and a plain resume does not bring it back. */
  killSwitch(): void {
    for (const t of this.tasks.filter((x) => x.status === "running" || x.status === "queued")) {
      this.enterPause(t, "kill_switch", "Fermato dal pulsante di emergenza: riaccendi l'assistente dalle impostazioni e poi riprendilo.", "Fermato dal pulsante di emergenza.");
    }
  }

  // ---- the confirmations of the tasks, next to the ones of the plain runs

  waitingApprovals(): ApprovalRequest[] {
    this.settle();
    return structuredClone(this.tasks.flatMap((t) => (t.waiting_approval ? [t.waiting_approval] : [])));
  }

  owns(approvalId: string): boolean {
    return this.tasks.some((t) => t.waiting_approval?.id === approvalId);
  }

  /** The person's answer: the task goes on either way (a refusal is not an error), and may ask again. */
  respond(approvalId: string, decision: "allow" | "deny"): ApprovalRequest | undefined {
    const t = this.tasks.find((x) => x.waiting_approval?.id === approvalId);
    if (!t?.waiting_approval) return undefined;
    const req = t.waiting_approval;
    t.waiting_approval = undefined;
    t.progress = {
      ...t.progress, turns: t.progress.turns + 1, last_tool: req.tool, updated_at: new Date().toISOString(),
      last_text: decision === "allow" ? "Fatto: ho eseguito quello che mi hai consentito." : "Ok, quello non lo faccio: vado avanti con il resto.",
    };
    this.touch(t, decision === "allow" ? `Hai consentito «${req.tool}».` : `Hai negato «${req.tool}»: il lavoro va avanti senza.`);
    if (this.script.get(t.id)?.length && t.status === "running") this.timer(t.id).askAt = Date.now() + READ_MS;
    return req;
  }
}
