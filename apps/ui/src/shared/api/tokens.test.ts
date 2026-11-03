import { describe, expect, it } from "vitest";
import { serverOrigin, storedToken, storeToken, tokenKey } from "./tokens";

function mem(init: Record<string, string> = {}) {
  const m = new Map(Object.entries(init));
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v), m };
}

describe("tokens bound to the issuing origin", () => {
  const page = "http://192.168.1.42:7777/";
  it("resolves the server origin", () => {
    expect(serverOrigin("", page)).toBe("http://192.168.1.42:7777");
    expect(serverOrigin("https://evil.example/x", page)).toBe("https://evil.example");
    expect(serverOrigin("http://[::1", page)).toBeNull();
  });
  it("never hands a token to another origin", () => {
    const s = mem({ "aiw.token": "legacy", [tokenKey("http://192.168.1.42:7777")]: "mine" });
    expect(storedToken(s, "http://192.168.1.42:7777", "http://192.168.1.42:7777")).toBe("mine");
    expect(storedToken(s, "https://evil.example", "http://192.168.1.42:7777")).toBeNull();
  });
  it("keeps the legacy unscoped token for the page's own server only", () => {
    const s = mem({ "aiw.token": "legacy" });
    expect(storedToken(s, "http://192.168.1.42:7777", "http://192.168.1.42:7777")).toBe("legacy");
    expect(storedToken(s, "http://10.0.0.2:7777", "http://192.168.1.42:7777")).toBeNull();
  });
  it("stores per origin", () => {
    const s = mem();
    storeToken(s, "http://10.0.0.2:7777", "t");
    expect(s.m.get(tokenKey("http://10.0.0.2:7777"))).toBe("t");
  });
});
