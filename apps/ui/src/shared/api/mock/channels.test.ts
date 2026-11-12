import { describe, expect, it } from "vitest";
import { MockTransport } from "./index";
import { SECRET_MASK } from "../types";

describe("channels in the demo", () => {
  it("creates a channel off, never shows its secrets and enables it only when it is ready", async () => {
    const t = new MockTransport();
    const c = await t.createChannel({ kind: "telegram", name: "Casa", secrets: { bot_token: "1:SEGRETO" } });
    expect(c.enabled).toBe(false);
    expect(c.mode).toBe("read_only");
    expect(c.status.state).toBe("off");
    expect(JSON.stringify(await t.channels())).not.toContain("SEGRETO");
    expect(c.secrets.bot_token).toBe(SECRET_MASK);
    const on = await t.updateChannel(c.id, { enabled: true, secrets: { bot_token: SECRET_MASK } });
    expect(on.status.state).toBe("connected");
    expect((await t.updateChannel(c.id, { enabled: false })).status.state).toBe("off");
  });

  it("refuses what the engine refuses, in plain Italian", async () => {
    const t = new MockTransport();
    await expect(t.createChannel({ kind: "fax" as never })).rejects.toMatchObject({ status: 400 });
    const s = await t.createChannel({ kind: "slack", secrets: { bot_token: "xoxb-1", app_token: "xapp-1" } });
    await expect(t.updateChannel(s.id, { enabled: true })).rejects.toThrow(/chi può parlare/);
    await expect(t.updateChannel(s.id, { allowed: ["pippo"] })).rejects.toThrow(/identificativo Slack/);
    expect((await t.updateChannel(s.id, { allowed: ["U024BE7LH"], enabled: true })).enabled).toBe(true);
    const m = await t.createChannel({ kind: "email", allowed: ["Mario@Example.it"] });
    expect(m.allowed).toEqual(["mario@example.it"]);
    await expect(t.updateChannel(m.id, { enabled: true })).rejects.toThrow(/posta in arrivo/);
    await expect(t.pairChannel(m.id)).rejects.toMatchObject({ status: 400 });
  });

  it("gives a ten-minute pairing code for Telegram and removes channels", async () => {
    const t = new MockTransport();
    const c = await t.createChannel({ kind: "telegram" });
    const p = await t.pairChannel(c.id);
    expect(p.command).toBe(`/start ${p.code}`);
    expect(Date.parse(p.expires_at) - Date.now()).toBeGreaterThan(9 * 60_000);
    expect((await t.testChannel(c.id)).ok).toBe(false);
    await t.deleteChannel(c.id);
    await expect(t.channelStatus(c.id)).rejects.toMatchObject({ status: 404 });
  });
});

describe("IMAP accounts in the demo", () => {
  it("keeps the password and shows it masked; the mask keeps it", async () => {
    const t = new MockTransport();
    const saved = await t.setMailAccounts([{ id: "Casa", host: "imap.example.com", port: 993, username: "mario", password: "pw", folders: [] }]);
    expect(saved[0]).toMatchObject({ id: "casa", password: SECRET_MASK, folders: ["INBOX"] });
    const again = await t.setMailAccounts([{ ...saved[0], host: "altro.example.com" }]);
    expect(again[0].password).toBe(SECRET_MASK);
    expect((await t.testMailAccount(again[0])).ok).toBe(true);
    expect((await t.testMailAccount({ ...again[0], password: "" })).detail).toMatch(/password per le app/);
    await expect(t.setMailAccounts([{ id: "", host: "", port: 993, username: "x", password: "p", folders: [] }])).rejects.toMatchObject({ status: 400 });
    expect(await t.setMailAccounts([])).toEqual([]);
  });
});
