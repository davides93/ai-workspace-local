import { describe, expect, it } from "vitest";
import { MockTransport } from "../../../shared/api/mock";
import { securityIt as itMessages } from "../../../shared/i18n/messages/it/security";
import { securityEn as enMessages } from "../../../shared/i18n/messages/en/security";

describe("secrets at rest: the demo and the words", () => {
  it("the demo transport answers with counts and words only", async () => {
    const s = await new MockTransport().secretsAtRest();
    expect(s.state).toBe("active");
    expect(Object.keys(s).sort()).toEqual(["backend", "encrypted", "plain", "state", "unreadable"]);
  });

  it("every state has a name and a note in both languages", () => {
    for (const k of ["active", "off", "unavailable", "key_lost", "not_used"]) {
      expect(itMessages).toHaveProperty(`security.segreti_stato_${k}`);
      expect(enMessages).toHaveProperty(`security.segreti_stato_${k}`);
    }
    for (const k of ["active", "plain", "off", "key_lost"]) {
      expect(itMessages).toHaveProperty(`security.segreti_note_${k}`);
      expect(enMessages).toHaveProperty(`security.segreti_note_${k}`);
    }
  });
});
