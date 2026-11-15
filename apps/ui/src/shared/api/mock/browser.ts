import { ApiError } from "../http";
import type { BrowserControlAction, BrowserDialogReply, BrowserFrame, BrowserHandover, BrowserInput, BrowserState } from "../types";

// The controlled browser of the demo: a fake page (a quotes table, a sign-in form, a members' area) drawn as a small SVG, and the same rules as the
// engine (`aiw-tools` `browser::Control`): who drives it, input only while the person drives and nothing of the agent is in flight, a window of the
// site that blocks the frame (409), the state after a handover. Nothing the person types is kept: only how many characters a field has,
// drawn as dots, so that the frame changes while typing and no value exists anywhere.

type Screen = "quotes" | "login" | "area";
type FocusId = "email" | "password" | "submit";

const FOCUS_ORDER: readonly FocusId[] = ["email", "password", "submit"];
const FOCUS_TEXT: Record<FocusId, string> = {
  email: "campo di testo «Email»",
  password: "campo password «Password»",
  submit: "pulsante «Accedi»",
};
/** Where the fields are on the sign-in page, as fractions of the frame (top, bottom). */
const FIELD_Y: Record<FocusId, [number, number]> = { email: [0.3, 0.44], password: [0.5, 0.64], submit: [0.7, 0.86] };

const W = 1000;
const H = 500;
const HOST = "fornitori.example.com";
const TITLES: Record<Screen, string> = { quotes: "Preventivi fornitori", login: "Accedi · Fornitori", area: "Area riservata · Fornitori" };

const closedState = (): BrowserState => ({ running: false, headless: false, mode: "agent", agent_busy: false, stale: false, handover: null, page: null, dialog: null, focus: null });

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c);

export class BrowserDemo {
  private s: BrowserState = closedState();
  private screen: Screen = "quotes";
  private focusId: FocusId | null = null;
  /** How many characters each field holds: never what they are. */
  private len: Record<"email" | "password", number> = { email: 0, password: 0 };

  state(): BrowserState {
    return { ...structuredClone(this.s), focus: this.s.mode === "human" && !this.s.agent_busy && !this.s.dialog && this.focusId ? FOCUS_TEXT[this.focusId] : null };
  }

  /** The agent opens (or navigates) the page: a browser runs from now on. */
  open(screen: Screen): void {
    this.screen = screen;
    this.focusId = null;
    this.len = { email: 0, password: 0 };
    this.s = { ...this.s, running: true, page: { title: TITLES[screen], host: HOST, secure: true } };
  }

  /** The agent is in the middle of an action (its result is still to come): the person's input waits. */
  setBusy(busy: boolean): void {
    this.s.agent_busy = busy;
  }

  /** Resolves when the agent may act (nobody paused it, the person does not drive): every action of the agent goes through here. */
  async gate(cancelled: () => boolean = () => false): Promise<void> {
    while (this.s.mode !== "agent" && !cancelled()) await new Promise((r) => setTimeout(r, 100));
  }

  beginHandover(h: BrowserHandover): void {
    this.s = { ...this.s, mode: "human", handover: h };
  }

  /** The browser goes back to the agent, with its references stale (the person drove in between). Idempotent. */
  endHandover(): void {
    const wasHuman = this.s.mode === "human";
    this.s = { ...this.s, mode: "agent", handover: null, stale: this.s.stale || wasHuman, dialog: null };
  }

  /** The run ended or was stopped: a request for help of that run is over. */
  releaseRun(runId: string): void {
    if (this.s.handover?.run_id === runId) this.endHandover();
  }

  handover(): BrowserHandover | null {
    return this.s.handover;
  }

  /** `give_back` answers the question that carries a handover through `answer`, like the engine. */
  control(action: BrowserControlAction, answerHandover: (questionId: string) => void): BrowserState {
    const mode = this.s.mode;
    if (action === "pause") {
      if (mode === "agent") this.s.mode = "paused";
    } else if (action === "resume") {
      if (mode === "human") throw new ApiError(409, "il browser è in mano all'utente: restituiscilo con «Restituisci all'agente»");
      if (mode === "paused") this.s.mode = "agent";
    } else if (action === "take") {
      this.s.mode = "human";
    } else if (action === "give_back") {
      const h = this.s.handover;
      if (h) answerHandover(h.question_id);
      this.endHandover();
    } else {
      throw new ApiError(400, `azione \`${String(action)}\` non valida: usa pause, resume, take o give_back`);
    }
    return this.state();
  }

  frame(): BrowserFrame {
    if (!this.s.running) return { running: false };
    if (this.s.dialog) throw new ApiError(409, "una finestra del sito aspetta una risposta: rispondi prima a quella");
    return { running: true, image: `data:image/svg+xml;utf8,${encodeURIComponent(this.svg())}`, width: W, height: H };
  }

  input(input: BrowserInput): void {
    if (this.s.mode !== "human") throw new ApiError(409, "prima prendi il controllo del browser");
    if (this.s.agent_busy) throw new ApiError(409, "l'agente sta finendo un'azione: riprova tra un istante");
    if (this.s.dialog) throw new ApiError(409, "una finestra del sito aspetta una risposta: rispondi prima a quella");
    if (input.type === "text") {
      if (Array.from(input.text).length > 512) throw new ApiError(400, "il testo è troppo lungo: al massimo 512 caratteri");
      this.typeChars(Array.from(input.text).length);
    } else if (input.type === "key") {
      this.key(input.key, input.shift);
    } else if (input.type === "click") {
      this.click(input.x, input.y);
    }
    // A scroll changes nothing on this small page.
  }

  answerDialog(reply: BrowserDialogReply): void {
    // The same turn rules as the engine's `human_begin`, then the window: none waiting is a 409 too (a second answer, or one given elsewhere).
    if (this.s.mode !== "human") throw new ApiError(409, "prima prendi il controllo del browser");
    if (this.s.agent_busy) throw new ApiError(409, "l'agente sta finendo un'azione: riprova tra un istante");
    if (!this.s.dialog) throw new ApiError(409, "nessuna finestra del sito aspetta una risposta");
    const accepted = reply.action === "accept";
    this.s.dialog = null;
    // «Esci» asked whether to leave the members' area.
    if (accepted && this.screen === "area") this.open("login");
  }

  // ---------------------------------------------------------------- the fake page

  private typeChars(n: number): void {
    if (this.screen !== "login" || (this.focusId !== "email" && this.focusId !== "password")) return;
    this.len[this.focusId] = Math.max(0, this.len[this.focusId] + n);
  }

  private key(key: string, shift: boolean): void {
    if (this.screen !== "login") return;
    if (key === "Tab") {
      const i = this.focusId ? FOCUS_ORDER.indexOf(this.focusId) : -1;
      this.focusId = FOCUS_ORDER[(i + (shift ? -1 : 1) + FOCUS_ORDER.length) % FOCUS_ORDER.length];
    } else if (key === "Backspace") this.typeChars(-1);
    else if (key === "Enter") {
      if (this.focusId === "submit" || this.focusId === "password") this.signIn();
    } else if (Array.from(key).length === 1) this.typeChars(1);
  }

  private click(x: number, y: number): void {
    if (this.screen === "area") {
      // The «Esci» link, bottom right: the site asks first.
      if (x > 0.78 && y > 0.82) this.s.dialog = { kind: "confirm", message: "Vuoi davvero uscire dall'area riservata?", default_text: "" };
      return;
    }
    if (this.screen !== "login") return;
    const hit = FOCUS_ORDER.find((f) => y >= FIELD_Y[f][0] && y <= FIELD_Y[f][1] && x > 0.2 && x < 0.8) ?? null;
    this.focusId = hit;
    if (hit === "submit") this.signIn();
  }

  private signIn(): void {
    // The demo accepts any sign-in that typed something in both fields.
    if (this.len.email > 0 && this.len.password > 0) {
      this.screen = "area";
      this.focusId = null;
      this.s.page = { title: TITLES.area, host: HOST, secure: true };
    }
  }

  private svg(): string {
    const box = (id: "email" | "password", y: number, label: string) => {
      const on = this.focusId === id;
      const dots = "•".repeat(Math.min(this.len[id], 24));
      return `<text x="200" y="${y - 12}" font-size="18" fill="#444">${label}</text><rect x="200" y="${y}" width="600" height="52" rx="6" fill="#fff" stroke="${on ? "#c77700" : "#999"}" stroke-width="${on ? 4 : 2}"/><text x="216" y="${y + 34}" font-size="26" fill="#222">${esc(dots)}</text>`;
    };
    let body = "";
    if (this.screen === "login") {
      const on = this.focusId === "submit";
      body = `<text x="200" y="110" font-size="34" font-weight="700" fill="#222">Accedi</text>${box("email", 170, "Email")}${box("password", 270, "Password")}<rect x="200" y="360" width="200" height="52" rx="6" fill="#2c5e8a" stroke="${on ? "#c77700" : "#2c5e8a"}" stroke-width="${on ? 5 : 2}"/><text x="258" y="394" font-size="24" fill="#fff">Accedi</text>`;
    } else if (this.screen === "area") {
      body = `<text x="60" y="120" font-size="34" font-weight="700" fill="#222">Area riservata</text><text x="60" y="170" font-size="22" fill="#444">Benvenuto: 3 preventivi in attesa di conferma.</text><rect x="60" y="210" width="880" height="44" fill="#e8ecef"/><text x="76" y="240" font-size="20" fill="#222">Rossi Srl · 12.400 € · 45 gg</text><rect x="60" y="262" width="880" height="44" fill="#f3f5f6"/><text x="76" y="292" font-size="20" fill="#222">Nordtech · 14.950 € · 20 gg</text><text x="830" y="460" font-size="22" fill="#2c5e8a" text-decoration="underline">Esci</text>`;
    } else {
      body = `<text x="60" y="120" font-size="34" font-weight="700" fill="#222">Preventivi fornitori</text><rect x="60" y="160" width="880" height="44" fill="#e8ecef"/><text x="76" y="190" font-size="20" fill="#222">Rossi Srl · 12.400 € · 45 gg</text><rect x="60" y="212" width="880" height="44" fill="#f3f5f6"/><text x="76" y="242" font-size="20" fill="#222">Nordtech · 14.950 € · 20 gg</text><rect x="60" y="264" width="880" height="44" fill="#e8ecef"/><text x="76" y="294" font-size="20" fill="#222">Alfa Impianti · 13.700 € · 30 gg</text>`;
    }
    return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" font-family="Arial, Helvetica, sans-serif"><rect width="${W}" height="${H}" fill="#f7f8f6"/><rect width="${W}" height="48" fill="#dfe3df"/><text x="16" y="31" font-size="18" fill="#333">https://${HOST}</text>${body}</svg>`;
  }
}
