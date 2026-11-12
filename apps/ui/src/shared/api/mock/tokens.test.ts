import { describe, expect, it } from "vitest";
import { MockTransport } from "./index";

describe("mock access tokens and TLS", () => {
  it("creates a token shown once, then lists only its prefix", async () => {
    const t = new MockTransport();
    const c = await t.createApiToken({ name: "Editor", kind: "gateway", expires_in_days: 30 });
    expect(c.token.startsWith("aiw_")).toBe(true);
    expect(c.scopes).toEqual(["gateway"]);
    const list = await t.apiTokens();
    const row = list.find((x) => x.id === c.id)!;
    expect(JSON.stringify(list)).not.toContain(c.token);
    expect(row.expires_at).toBeTruthy();
    await expect(t.createApiToken({ name: " " })).rejects.toThrow(/nome/);
  });
  it("revokes, rotates and protects the owner token", async () => {
    const t = new MockTransport();
    const c = await t.createApiToken({ name: "App" });
    const r = await t.rotateApiToken(c.id);
    expect(r.id).not.toBe(c.id);
    expect((await t.apiTokens()).find((x) => x.id === c.id)?.revoked_at).toBeTruthy();
    await t.revokeApiToken(r.id);
    await expect(t.rotateApiToken(r.id)).rejects.toThrow(/revocato/);
    await expect(t.revokeApiToken("tok_owner")).rejects.toThrow();
  });
  it("needs a fingerprint to pair over https", async () => {
    const t = new MockTransport();
    await expect(t.pairPeer("https://192.168.1.31:7777", "123456")).rejects.toThrow(/impronta/);
    await expect(t.pairPeer("https://192.168.1.31:7777", "123456", "ab".repeat(32))).resolves.toBeTruthy();
    expect((await t.tlsStatus()).warning).toBe(true);
  });
});
