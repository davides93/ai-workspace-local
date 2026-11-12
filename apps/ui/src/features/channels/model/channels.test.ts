import { describe, expect, it } from "vitest";
import type { ChannelInfo } from "../../../shared/api/types";
import { SECRET_MASK } from "../../../shared/api/types";
import { accountProblem, diffPatch, emptyAccount, enableProblem, ignoredNote, kindInfo, minutesLeft, missingSecrets, parseFolders, secretsToSend, stateTone } from "./channels";

const base = (over: Partial<ChannelInfo> = {}): ChannelInfo => ({
  id: "c1", kind: "telegram", name: "Casa", enabled: false, agent_id: "operator", mode: "read_only", allowed: [], briefing: false,
  secrets: { bot_token: "" }, settings: {}, status: { state: "off", detail: "", ignored: 0, handled: 0, conversations: 0 }, downgraded: false, ...over,
});

describe("guided set-up", () => {
  it("has plain Italian steps per kind, and says OAuth is not there for email", () => {
    expect(kindInfo("telegram").steps[0]).toMatch(/BotFather/);
    expect(kindInfo("slack").steps.join(" ")).toMatch(/message\.im/);
    expect(kindInfo("email").steps.join(" ")).toMatch(/password per le app/);
  });
});

describe("enableProblem", () => {
  it("asks for the token first, and Telegram needs no list (the code adds you)", () => {
    expect(missingSecrets(base())).toEqual(["Token del bot"]);
    expect(enableProblem(base())).toMatch(/Manca: Token del bot/);
    expect(enableProblem(base({ secrets: { bot_token: SECRET_MASK } }))).toBeNull();
  });
  it("needs a list for Slack and email", () => {
    const slack = base({ kind: "slack", secrets: { bot_token: SECRET_MASK, app_token: SECRET_MASK } });
    expect(enableProblem(slack)).toMatch(/chi può parlare/);
    expect(enableProblem({ ...slack, allowed: ["U024BE7LH"] })).toBeNull();
    const mail = base({ kind: "email", secrets: { imap_password: SECRET_MASK, smtp_password: "" }, allowed: ["a@b.it"] });
    expect(enableProblem(mail)).toMatch(/smtp_password|Password della posta in uscita/);
    const ready = { ...mail, secrets: { imap_password: SECRET_MASK, smtp_password: SECRET_MASK } };
    expect(enableProblem(ready)).toMatch(/server e l'utente/);
    expect(enableProblem({ ...ready, settings: { imap_host: "h", imap_user: "u" } })).toMatch(/uscita/);
    expect(enableProblem({ ...ready, settings: { imap_host: "h", imap_user: "u", smtp_host: "s" } })).toBeNull();
  });
});

describe("diffPatch", () => {
  const form = (c: ChannelInfo) => ({ name: c.name, mode: c.mode, allowed: c.allowed, settings: {}, typedSecrets: {} });
  it("is empty when nothing changed and never sends an untouched secret", () => {
    const c = base({ secrets: { bot_token: SECRET_MASK } });
    expect(diffPatch(c, form(c))).toEqual({});
    expect(diffPatch(c, { ...form(c), typedSecrets: { bot_token: "  " } })).toEqual({});
  });
  it("sends what changed", () => {
    const c = base({ secrets: { bot_token: SECRET_MASK }, allowed: ["1"] });
    expect(diffPatch(c, { ...form(c), name: " Altro ", mode: "ask", allowed: ["1", "2"], typedSecrets: { bot_token: " nuovo " } })).toEqual({
      name: "Altro", mode: "ask", allowed: ["1", "2"], secrets: { bot_token: "nuovo" },
    });
    const mail = base({ kind: "email", settings: { imap_host: "a" } });
    expect(diffPatch(mail, { ...form(mail), settings: { imap_host: "a", smtp_host: " s " } })).toEqual({ settings: { smtp_host: "s" } });
    expect(secretsToSend({ a: "", b: " x " })).toEqual({ b: "x" });
  });
});

describe("small helpers", () => {
  it("counts the ignored strangers in words", () => {
    expect(ignoredNote(0)).toBe("Nessun messaggio ignorato.");
    expect(ignoredNote(1)).toBe("1 messaggio ignorato da sconosciuti.");
    expect(ignoredNote(7)).toBe("7 messaggi ignorati da sconosciuti.");
  });
  it("shows the state with a tone", () => {
    expect(["connected", "connecting", "error", "off", "unavailable"].map((s) => stateTone(s as never))).toEqual(["ok", "run", "crit", "off", "warn"]);
  });
  it("counts the minutes left of a pairing code", () => {
    const now = Date.parse("2026-01-01T10:00:00Z");
    expect(minutesLeft("2026-01-01T10:09:30Z", now)).toBe(10);
    expect(minutesLeft("2026-01-01T10:00:20Z", now)).toBe(1);
    expect(minutesLeft("2026-01-01T09:59:00Z", now)).toBe(0);
    expect(minutesLeft("boh", now)).toBe(0);
  });
});

describe("mail accounts", () => {
  it("checks the form in plain words", () => {
    const a = { ...emptyAccount(), host: "imap.example.com", username: "mario", password: "pw" };
    expect(accountProblem(a)).toBeNull();
    expect(accountProblem({ ...a, host: " " })).toMatch(/server/);
    expect(accountProblem({ ...a, username: "" })).toMatch(/utente/);
    expect(accountProblem({ ...a, port: 0 })).toMatch(/porta/);
    expect(accountProblem({ ...a, password: "" })).toMatch(/password per le app/);
    expect(accountProblem({ ...a, password: SECRET_MASK })).toBeNull();
  });
  it("reads folders one per line or with commas", () => {
    expect(parseFolders("INBOX, Lavoro\n\n Fatture ")).toEqual(["INBOX", "Lavoro", "Fatture"]);
  });
});
