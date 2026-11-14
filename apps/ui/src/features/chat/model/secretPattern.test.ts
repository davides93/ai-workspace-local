import { describe, expect, it } from "vitest";
import { looksLikeSecret } from "./secretPattern";

// Obviously fake values, built from parts so that no scanner takes them for real credentials.
const FAKE = {
  openai: `${"sk"}-TEST_ONLY_not_a_real_key_0000`,
  github: `${"gh"}p_TESTONLYNOTAREALTOKEN0000`,
  githubOauth: `${"gh"}o_TESTONLYNOTAREALTOKEN0000`,
  githubFine: `${"github"}_pat_TESTONLY_not_a_real_token_0000`,
  aws: `${"AK"}IA${"TESTONLYFAKE0000"}`,
  slack: `${"xox"}b-TEST-ONLY-0000-FAKE`,
  bearer: "Bearer TESTONLY.fake.token-0000",
  privateKey: "-----BEGIN RSA PRIVATE KEY-----\nTESTONLY\n",
};

describe("looksLikeSecret", () => {
  it("recognises every shape the engine masks", () => {
    for (const [name, value] of Object.entries(FAKE)) expect(looksLikeSecret(value), name).toBe(true);
  });
  it("recognises them inside a sentence, anywhere in the text", () => {
    expect(looksLikeSecret(`usa questa chiave ${FAKE.openai} per il servizio`)).toBe(true);
    expect(looksLikeSecret(`curl -H "Authorization: ${FAKE.bearer}" https://example.test`)).toBe(true);
    expect(looksLikeSecret(`riga uno\nriga due ${FAKE.aws}\nriga tre`)).toBe(true);
    expect(looksLikeSecret(`OPENAI_API_KEY=${FAKE.openai}`)).toBe(true);
    expect(looksLikeSecret(`("${FAKE.openai}")`)).toBe(true);
  });
  it("takes the scheme of Bearer without regard to case, like the engine", () => {
    expect(looksLikeSecret("bearer TESTONLYFAKETOKEN0000")).toBe(true);
    expect(looksLikeSecret("BEARER TESTONLYFAKETOKEN0000")).toBe(true);
  });
  it("is quiet on ordinary text, short values and look-alikes", () => {
    expect(looksLikeSecret("")).toBe(false);
    expect(looksLikeSecret("Scarica un modello adatto a questo computer")).toBe(false);
    expect(looksLikeSecret("la mia email è anna@example.test")).toBe(false);
    expect(looksLikeSecret("sk-corto")).toBe(false);
    // `sk-` at the end of a word (disk, task, risk) is not a key, even when a long hyphenated name follows it.
    expect(looksLikeSecret("task-force-ricerca-di-mercato-2026")).toBe(false);
    expect(looksLikeSecret("pulisci disk-usage-analyzer-tool-vecchio")).toBe(false);
    expect(looksLikeSecret("ghp_corto")).toBe(false);
    expect(looksLikeSecret("AKIA-non-abbastanza-lungo")).toBe(false);
    expect(looksLikeSecret("AKIAtestonlyfake0000")).toBe(false);
    expect(looksLikeSecret("xoxb-corto")).toBe(false);
    expect(looksLikeSecret("Bearer corto")).toBe(false);
    expect(looksLikeSecret("-----BEGIN CERTIFICATE-----")).toBe(false);
  });
  it("does not cover Hugging Face tokens (the engine does not mask them either)", () => {
    expect(looksLikeSecret(`${"hf"}_TEST_ONLY_not_a_real_token_0000`)).toBe(false);
  });
});
