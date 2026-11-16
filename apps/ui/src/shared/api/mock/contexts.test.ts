import { beforeEach, describe, expect, it } from "vitest";
import { MockTransport } from "./index";
import { resetContexts } from "./contexts";
import { sessions } from "./fixtures";
import type { ChannelInfo } from "../types";

// The demo applies the rules of the engine (crates/aiw-engine/src/channels/context.rs): the owner links accounts one by one,
// nothing is merged by itself, and what could leak a private history is refused.

async function channels(t: MockTransport): Promise<{ tg: ChannelInfo; mail: ChannelInfo }> {
  for (const c of await t.channels()) await t.deleteChannel(c.id);
  const tg = await t.createChannel({ kind: "telegram", name: "Telegram", mode: "ask", allowed: ["111", "-100777"], secrets: { bot_token: "1:x" } });
  const mail = await t.createChannel({ kind: "email", name: "Posta", mode: "ask", allowed: ["Mario@Example.it"], settings: { imap_host: "h" } });
  return { tg, mail };
}

describe("shared contexts in the demo", () => {
  beforeEach(() => resetContexts());

  it("links two accounts on the owner's say-so, with a shared session of its own, and never merges anything", async () => {
    const t = new MockTransport();
    const { tg, mail } = await channels(t);
    expect(await t.contexts()).toEqual([]);
    const ctx = await t.createContext({ name: "  Mario,   ovunque ", members: [{ channel_id: tg.id, sender: "111" }, { channel_id: mail.id, sender: "MARIO@example.it" }] });
    expect(ctx.name).toBe("Mario, ovunque");
    expect(ctx.members.map((m) => m.sender)).toEqual(["111", "mario@example.it"]);
    expect(ctx.mode).toBe("ask");
    const shared = sessions.find((s) => s.id === ctx.session_id);
    expect(shared?.channel).toBe("shared");
    expect(shared?.agent_id).toMatch(/^channel:ask:/);
    expect(ctx.members_info.every((m) => m.problem === undefined)).toBe(true);
    // The context is never freer than its most restricted channel.
    await t.updateChannel(mail.id, { mode: "read_only" });
    expect((await t.contexts())[0].mode).toBe("read_only");
  });

  it("refuses what could leak a private history, in plain Italian", async () => {
    const t = new MockTransport();
    const { tg, mail } = await channels(t);
    await expect(t.createContext({ name: "x", members: [] })).rejects.toThrow(/almeno un account/);
    await expect(t.createContext({ name: "x", members: [{ channel_id: tg.id, sender: "222" }] })).rejects.toThrow(/non è nella lista/);
    await expect(t.createContext({ name: "x", members: [{ channel_id: tg.id, sender: "-100777" }] })).rejects.toThrow(/gruppo/);
    await expect(t.createContext({ name: "x", members: [{ channel_id: tg.id, sender: "111" }, { channel_id: tg.id, sender: "111" }] })).rejects.toThrow(/compare due volte/);
    await t.updateChannel(mail.id, { settings: { require_auth: false } });
    await expect(t.createContext({ name: "x", members: [{ channel_id: mail.id, sender: "mario@example.it" }] })).rejects.toThrow(/controllo del mittente/);
    await t.updateChannel(mail.id, { settings: { require_auth: true } });
    await t.updateChannel(mail.id, { agent_id: "coder" });
    await expect(t.createContext({ name: "x", members: [{ channel_id: tg.id, sender: "111" }, { channel_id: mail.id, sender: "mario@example.it" }] })).rejects.toThrow(/assistenti diversi/);
    await t.updateChannel(mail.id, { agent_id: "operator" });
    await t.createContext({ name: "Uno", members: [{ channel_id: tg.id, sender: "111" }] });
    await expect(t.createContext({ name: "Due", members: [{ channel_id: tg.id, sender: "111" }] })).rejects.toMatchObject({ status: 409 });
    await expect(t.createContext({ name: " ", members: [{ channel_id: mail.id, sender: "mario@example.it" }] })).rejects.toThrow(/nome/);
  });

  it("an account leaves when it is taken off the list or its channel is deleted, and unlinking keeps the conversation", async () => {
    const t = new MockTransport();
    const { tg, mail } = await channels(t);
    const ctx = await t.createContext({ name: "Mario", members: [{ channel_id: tg.id, sender: "111" }, { channel_id: mail.id, sender: "mario@example.it" }] });
    await t.updateChannel(tg.id, { allowed: ["-100777"] });
    expect((await t.contexts())[0].members.map((m) => m.channel_id)).toEqual([mail.id]);
    await t.deleteChannel(mail.id);
    expect(await t.contexts()).toEqual([]);
    expect(sessions.some((s) => s.id === ctx.session_id)).toBe(true);
    await expect(t.deleteContext("ctx_nope")).rejects.toMatchObject({ status: 404 });
  });
});
