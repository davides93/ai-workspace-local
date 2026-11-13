import { describe, expect, it } from "vitest";
import { describeExpiry, describeLastUse, exampleFor, tokenState } from "./tokens";

describe("token state", () => {
  const now = Date.parse("2026-06-10T10:00:00Z");
  it("tells active, revoked and expired apart", () => {
    expect(tokenState({}, now)).toBe("active");
    expect(tokenState({ expires_at: "2026-07-01T00:00:00Z" }, now)).toBe("active");
    expect(tokenState({ expires_at: "2026-06-01T00:00:00Z" }, now)).toBe("expired");
    expect(tokenState({ revoked_at: "2026-06-02T00:00:00Z", expires_at: "2099-01-01T00:00:00Z" }, now)).toBe("revoked");
  });
  it("describes use and expiry in plain Italian", () => {
    expect(describeLastUse({})).toBe("mai usato");
    expect(describeLastUse({ last_used_at: "2026-06-10T10:00:00Z", last_used_ip: "192.168.1.5" })).toContain("da 192.168.1.5");
    expect(describeExpiry({})).toBe("non scade");
  });
  it("shows the OPENAI_BASE_URL example", () => {
    expect(exampleFor("aiw_x")).toBe("export OPENAI_BASE_URL=http://127.0.0.1:7777/v1\nexport OPENAI_API_KEY=aiw_x");
  });
});
