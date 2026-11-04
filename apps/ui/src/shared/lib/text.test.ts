import { describe, expect, it } from "vitest";
import { matchesQuery, normalizeText } from "./text";

describe("text search", () => {
  it("lowercases and strips accents", () => {
    expect(normalizeText("Attività è già")).toBe("attivita e gia");
  });
  it("matches when every word of the query is somewhere in the text", () => {
    expect(matchesQuery("Riordino PDF in Downloads", "pdf downloads")).toBe(true);
    expect(matchesQuery("Riordino PDF in Downloads", "pdf fornitori")).toBe(false);
  });
  it("ignores case, accents and extra spaces; an empty query matches everything", () => {
    expect(matchesQuery("Attività recente", "  ATTIVITA ")).toBe(true);
    expect(matchesQuery("qualsiasi cosa", "")).toBe(true);
    expect(matchesQuery("qualsiasi cosa", "   ")).toBe(true);
  });
});
