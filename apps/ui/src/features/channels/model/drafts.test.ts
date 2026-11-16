import { describe, expect, it } from "vitest";
import type { ChannelInfo, DraftStatus, EmailDraft } from "../../../shared/api/types";
import { canEdit, canResend, canSend, canStop, draftStatusLabel, draftTitle, isClosed, needsOwner, recipientsSummary, shortHash, visibleDrafts } from "./drafts";
import { linkableChannels, linkableSenders, pickedMembers, takenSenders } from "./contexts";

const draft = (over: Partial<EmailDraft> = {}): EmailDraft => ({
  id: "drf_1", status: "draft", channel_id: "c1", from: "a@casa.example", to: ["anna@cliente.it"], cc: [], bcc: [], subject: "Ciao", body: "Testo", attachments: [],
  created_by: "assistant", source: "chat", content_hash: "ab".repeat(32), revision: 1, created_at: "2026-11-15T10:00:00Z", updated_at: "2026-11-15T10:00:00Z", ...over,
});

describe("what the owner can do with a draft, by state", () => {
  const states: DraftStatus[] = ["draft", "sending", "sent", "failed", "uncertain", "rejected", "cancelled"];
  it("approve, edit, refuse and withdraw only before anything may have gone; an uncertain one only with an explicit «send anyway»", () => {
    const can = (f: (d: { status: DraftStatus }) => boolean) => states.filter((status) => f({ status }));
    expect(can(canSend)).toEqual(["draft", "failed"]);
    expect(can(canEdit)).toEqual(["draft", "failed"]);
    expect(can(canStop)).toEqual(["draft", "failed"]);
    expect(can(canResend)).toEqual(["uncertain"]);
    expect(can(isClosed)).toEqual(["sent", "rejected", "cancelled"]);
    expect(can(needsOwner)).toEqual(["draft", "sending", "failed", "uncertain"]);
  });
  it("has a label for every state, in plain Italian", () => {
    for (const s of states) expect(draftStatusLabel(s)).toMatch(/\S/);
    expect(draftStatusLabel("uncertain")).toMatch(/incerto/);
  });
});

describe("the list", () => {
  it("shows what needs the owner, newest first, and the closed ones only when asked for", () => {
    const list = [draft({ id: "a", created_at: "2026-11-15T09:00:00Z" }), draft({ id: "b", status: "sent", created_at: "2026-11-15T11:00:00Z" }), draft({ id: "c", status: "failed", created_at: "2026-11-15T12:00:00Z" })];
    expect(visibleDrafts(list, false).map((d) => d.id)).toEqual(["c", "a"]);
    expect(visibleDrafts(list, true).map((d) => d.id)).toEqual(["c", "b", "a"]);
  });
  it("names a draft without a subject, abbreviates the fingerprint and lists the recipients with the copies", () => {
    expect(draftTitle(draft({ subject: "  " }))).toBe("(senza oggetto)");
    expect(draftTitle(draft())).toBe("Ciao");
    expect(shortHash("ab".repeat(32))).toHaveLength(12);
    expect(recipientsSummary(draft({ to: ["a@x.it", "b@x.it"], cc: ["c@x.it"], bcc: ["d@x.it"] }))).toEqual({ to: "a@x.it, b@x.it", cc: ", in copia a c@x.it", bcc: ", in copia nascosta a d@x.it" });
    expect(recipientsSummary(draft({ to: [] })).to).toBe("nessun destinatario");
  });
});

const channel = (over: Partial<ChannelInfo>): ChannelInfo => ({
  id: "c", kind: "telegram", name: "C", enabled: true, agent_id: "operator", mode: "ask", allowed: [], briefing: false, secrets: {}, settings: {},
  status: { state: "connected", detail: "", ignored: 0, handled: 0, conversations: 0 }, downgraded: false, ...over,
});

describe("linking accounts", () => {
  it("offers the people on the list, never a Telegram group", () => {
    expect(linkableSenders(channel({ allowed: ["111", "-100777"] }))).toEqual(["111"]);
    expect(linkableSenders(channel({ kind: "email", allowed: ["a@x.it"] }))).toEqual(["a@x.it"]);
    expect(linkableChannels([channel({ id: "g", allowed: ["-1"] }), channel({ id: "p", allowed: ["5"] })]).map((c) => c.id)).toEqual(["p"]);
  });
  it("turns the picks of the form into members, in the order of the channels, and knows which are taken", () => {
    const a = channel({ id: "a", allowed: ["1"] });
    const b = channel({ id: "b", kind: "email", allowed: ["m@x.it"] });
    expect(pickedMembers([a, b], { b: "m@x.it", a: "1" })).toEqual([{ channel_id: "a", sender: "1" }, { channel_id: "b", sender: "m@x.it" }]);
    expect(pickedMembers([a, b], { a: "", b: "m@x.it" })).toEqual([{ channel_id: "b", sender: "m@x.it" }]);
    const taken = takenSenders([{ id: "x", name: "n", agent_id: "o", session_id: "s", created_at: "", members: [{ channel_id: "a", sender: "1" }], members_info: [], mode: "ask", session_title: "" }]);
    expect(taken.has("a|1") && !taken.has("b|m@x.it")).toBe(true);
  });
});
