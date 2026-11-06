import { describe, expect, it } from "vitest";
import { SECRET_MASK, type RemoteAgent } from "../../../shared/api/types";
import { blankRemote, hasToken, normalizeRemote, remoteIdFor, validateRemote } from "./remote";

const r = (patch: Partial<RemoteAgent> = {}): RemoteAgent => ({ ...blankRemote(), name: "Studio", url: "http://h:7777", ...patch });

describe("remote agents", () => {
  it("derives a unique id from the name, never one of the paired devices", () => {
    expect(remoteIdFor("PC dello studio", [])).toBe("pc-dello-studio");
    expect(remoteIdFor("Studio", [r({ id: "studio" })])).toBe("studio-2");
    expect(remoteIdFor("Peer Casa", [])).toBe("remoto-peer-casa");
    expect(remoteIdFor("???", [])).toBe("agente");
  });
  it("trims what is sent and drops an empty skill", () => {
    expect(normalizeRemote(r({ name: " X ", url: " http://h ", skill: "  ", description: " d " }))).toEqual({ id: "", name: "X", url: "http://h", token: "", description: "d" });
    expect(normalizeRemote(r({ skill: " operator " }))).toMatchObject({ skill: "operator" });
  });
  it("needs a name and an http(s) address", () => {
    expect(validateRemote(r())).toEqual([]);
    expect(validateRemote(r({ name: " ", url: "" })).map((i) => i.field)).toEqual(["name", "url"]);
    expect(validateRemote(r({ url: "ftp://h" }))).toEqual([{ field: "url", message: "L'indirizzo deve iniziare con http:// o https://." }]);
  });
  it("knows when a token is set, masked or not", () => {
    expect(hasToken(r({ token: SECRET_MASK }))).toBe(true);
    expect(hasToken(r({ token: "x" }))).toBe(true);
    expect(hasToken(r())).toBe(false);
  });
});
