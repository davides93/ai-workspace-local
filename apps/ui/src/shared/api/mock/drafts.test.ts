import { beforeEach, describe, expect, it } from "vitest";
import { MockTransport } from "./index";
import { outbox, resetDrafts, seedDraft } from "./drafts";

// The demo applies the rules of the engine (crates/aiw-engine/src/channels/drafts.rs): nothing leaves without the approval of
// the content the owner saw, once only, and a refused or cancelled draft never leaves.

async function withChannel(t: MockTransport) {
  // The demo keeps its channels between tests: start from none, so the draft goes through this one.
  for (const c of await t.channels()) await t.deleteChannel(c.id);
  const ch = await t.createChannel({
    kind: "email", name: "Posta", allowed: ["mario@example.it"], secrets: { imap_password: "x", smtp_password: "y" },
    settings: { imap_host: "h", imap_user: "u", smtp_host: "s", from: "assistente@casa.example" },
  });
  await t.updateChannel(ch.id, { enabled: true });
  return ch;
}

describe("email drafts in the demo", () => {
  beforeEach(() => resetDrafts());

  it("needs an email channel, validates what it is given and keeps hidden copies out of the way", async () => {
    const t = new MockTransport();
    await expect(t.createEmailDraft({ to: ["anna@cliente.it"] })).rejects.toThrow(/canale di tipo Email/);
    const ch = await withChannel(t);
    for (const bad of ["senza-chiocciola", "a b@x.it", "a@x.it,b@x.it"]) await expect(t.createEmailDraft({ to: [bad] })).rejects.toMatchObject({ status: 400 });
    await expect(t.createEmailDraft({ to: Array.from({ length: 21 }, (_, i) => `p${i}@x.it`) })).rejects.toThrow(/al massimo 20/);
    const d = await t.createEmailDraft({ to: ["Anna@Cliente.it", "anna@cliente.it"], bcc: ["archivio@x.it"], subject: "Preventivo\n x", body: "Testo", attachments: ["docs/p.pdf"] });
    expect(d.status).toBe("draft");
    expect(d.to).toEqual(["Anna@Cliente.it"]);
    expect(d.subject).toBe("Preventivo x");
    expect(d.from).toBe("assistente@casa.example");
    expect(d.channel_id).toBe(ch.id);
    expect(d.attachments[0]).toMatchObject({ name: "p.pdf", mime: "application/pdf" });
    expect(d.content_hash).toHaveLength(64);
    // Attachments come from the workspace only.
    for (const out of ["/etc/passwd", "~/x", "../x", "C:/x"]) await expect(t.createEmailDraft({ to: ["a@x.it"], attachments: [out] })).rejects.toMatchObject({ status: 403 });
  });

  it("is sent only with the hash of the content that was shown, and once", async () => {
    const t = new MockTransport();
    await withChannel(t);
    const d = await t.createEmailDraft({ to: ["anna@cliente.it"], subject: "Ciao", body: "Testo" });
    await expect(t.sendEmailDraft(d.id, { approved_hash: "0".repeat(64) })).rejects.toMatchObject({ status: 409 });
    expect(outbox).toHaveLength(0);
    const changed = await t.updateEmailDraft(d.id, { body: "Altro testo" });
    expect(changed.revision).toBe(2);
    expect(changed.content_hash).not.toBe(d.content_hash);
    await expect(t.sendEmailDraft(d.id, { approved_hash: d.content_hash })).rejects.toThrow(/cambiata/);
    const sent = await t.sendEmailDraft(d.id, { approved_hash: changed.content_hash });
    expect(sent.status).toBe("sent");
    expect((await t.sendEmailDraft(d.id, { approved_hash: changed.content_hash })).status).toBe("sent");
    expect(outbox).toHaveLength(1);
    await expect(t.updateEmailDraft(d.id, { body: "x" })).rejects.toMatchObject({ status: 409 });
    const twin = await t.createEmailDraft({ to: ["anna@cliente.it"], subject: "Ciao", body: "Altro testo" });
    expect(twin.duplicate_of).toBe(d.id);
  });

  it("refusing or cancelling never sends, a closed draft can be removed, and a channel that is off stops the sending", async () => {
    const t = new MockTransport();
    const ch = await withChannel(t);
    const a = await t.createEmailDraft({ to: ["a@x.it"], subject: "A", body: "a" });
    const b = await t.createEmailDraft({ to: ["b@x.it"], subject: "B", body: "b" });
    expect((await t.rejectEmailDraft(a.id, "  il tono\n non va ")).note).toBe("il tono non va");
    expect((await t.cancelEmailDraft(b.id)).status).toBe("cancelled");
    for (const d of [a, b]) {
      await expect(t.sendEmailDraft(d.id, { approved_hash: d.content_hash })).rejects.toMatchObject({ status: 409 });
      await expect(t.rejectEmailDraft(d.id)).rejects.toMatchObject({ status: 409 });
    }
    expect(outbox).toHaveLength(0);
    await t.deleteEmailDraft(a.id);
    expect((await t.emailDrafts()).some((d) => d.id === a.id)).toBe(false);
    const c = await t.createEmailDraft({ to: ["c@x.it"], subject: "C", body: "c" });
    await expect(t.deleteEmailDraft(c.id)).rejects.toMatchObject({ status: 409 });
    await t.updateChannel(ch.id, { enabled: false });
    await expect(t.sendEmailDraft(c.id, { approved_hash: c.content_hash })).rejects.toThrow(/spento/);
    expect(outbox).toHaveLength(0);
    await expect(t.emailDrafts("boh" as never)).rejects.toMatchObject({ status: 400 });
  });

  it("an uncertain outcome is sent again only on an explicit say-so", async () => {
    const t = new MockTransport();
    await withChannel(t);
    const u = seedDraft({ to: ["a@x.it"], subject: "U", body: "u" }, "assistant", { status: "uncertain", note: "non so se è partita" });
    await expect(t.sendEmailDraft(u.id, { approved_hash: u.content_hash })).rejects.toThrow(/non so se il messaggio è partito/);
    expect(outbox).toHaveLength(0);
    expect((await t.sendEmailDraft(u.id, { approved_hash: u.content_hash, resend: true })).status).toBe("sent");
    expect(outbox).toHaveLength(1);
  });
});
