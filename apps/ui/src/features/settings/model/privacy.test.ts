import { describe, expect, it } from "vitest";
import { isLocalOnly, localOnlyWrites, privacyLines } from "./privacy";

const local = { "search.provider": "off", "update.auto_check": false, "update.auto_download": false, "proactive.level": "propose" };

describe("Solo locale", () => {
  it("is on only when search, update checks and the proactive web reading are all off", () => {
    expect(isLocalOnly(undefined)).toBe(false);
    expect(isLocalOnly(local)).toBe(true);
    expect(isLocalOnly({ ...local, "search.provider": "auto" })).toBe(false);
    expect(isLocalOnly({ ...local, "update.auto_check": true })).toBe(false);
    expect(isLocalOnly({ ...local, "update.auto_download": true })).toBe(false);
    expect(isLocalOnly({ ...local, "proactive.level": "act_readonly" })).toBe(false);
    expect(isLocalOnly({ ...local, "proactive.level": "off" })).toBe(true);
  });

  it("turning it on writes the three settings, and lowers a proactive assistant that reads the web", () => {
    expect(localOnlyWrites(true, { "proactive.level": "propose" })).toEqual([["search.provider", "off"], ["update.auto_check", false], ["update.auto_download", false]]);
    expect(localOnlyWrites(true, { "proactive.level": "act_readonly" })).toContainEqual(["proactive.level", "propose"]);
  });

  it("turning it off gives back the defaults of search and update checks only", () => {
    expect(localOnlyWrites(false, local)).toEqual([["search.provider", "auto"], ["update.auto_check", true]]);
  });

  it("lists what can leave the device, as the configuration stands", () => {
    const off = privacyLines(local);
    expect(off.map((l) => l.leaves)).toEqual([false, false, false, false]);
    const normal = privacyLines({ "search.provider": "auto", "update.auto_check": true, "proactive.level": "act_readonly" });
    expect(normal.map((l) => [l.id, l.leaves])).toEqual([["data", false], ["search", true], ["updates", true], ["proactive", true]]);
  });
});
