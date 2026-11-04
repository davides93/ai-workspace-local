import { describe, expect, it } from "vitest";
import { serviceBaseUrl } from "../../../shared/lib/baseUrl";
import { buildEndpoints, lanBaseUrl, TOKEN_PLACEHOLDER } from "./snippets";

describe("serviceBaseUrl", () => {
  it("uses the page origin for http(s) pages", () => {
    expect(serviceBaseUrl({ protocol: "http:", origin: "http://192.168.1.10:7777" })).toBe("http://192.168.1.10:7777");
    expect(serviceBaseUrl({ protocol: "https:", origin: "https://aiw.example" })).toBe("https://aiw.example");
  });
  it("falls back to the local daemon otherwise", () => {
    expect(serviceBaseUrl({ protocol: "file:", origin: "null" })).toBe("http://127.0.0.1:7777");
    expect(serviceBaseUrl({ protocol: "tauri:", origin: "tauri://localhost" })).toBe("http://127.0.0.1:7777");
  });
  it("never trusts the Tauri webview origin (http://tauri.localhost on Windows)", () => {
    expect(serviceBaseUrl({ native: true, protocol: "http:", origin: "http://tauri.localhost" })).toBe("http://127.0.0.1:7777");
  });
});

const BASE = "http://127.0.0.1:7777";
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor as new (body: string) => unknown;

describe("buildEndpoints", () => {
  const eps = buildEndpoints(`${BASE}/`);
  const byId = (id: string, list = eps) => list.find((e) => e.id === id)!;

  it("lists the five integrations with the documented paths", () => {
    expect(eps.map((e) => e.id)).toEqual(["openai", "anthropic", "mcp", "a2a", "webhook"]);
    expect(byId("openai").url).toBe(`${BASE}/v1`);
    expect(byId("anthropic").url).toBe(`${BASE}/v1/messages`);
    expect(byId("mcp").url).toBe(`${BASE}/mcp`);
    expect(byId("a2a").url).toBe(`${BASE}/a2a`);
    expect(byId("a2a").extra?.[0].url).toBe(`${BASE}/.well-known/agent-card.json`);
    expect(byId("webhook").url).toBe(`${BASE}/hooks`);
  });
  it("gives every endpoint one tab per language, built on the base URL", () => {
    for (const e of eps) {
      expect(e.snippets.length).toBeGreaterThanOrEqual(3);
      expect(new Set(e.snippets.map((s) => s.label)).size).toBe(e.snippets.length);
      for (const s of e.snippets) {
        expect(s.code).toContain(BASE);
        expect(s.code).not.toContain("7777//");
      }
    }
    const langs = (id: string) => byId(id).snippets.map((s) => s.lang);
    for (const id of ["openai", "anthropic", "a2a", "webhook"]) expect(langs(id)).toEqual(["curl", "python", "javascript"]);
    expect(langs("mcp")).toEqual(["config", "curl", "javascript"]);
  });
  it("shows the SDK each tab is about", () => {
    const label = (id: string, lang: string) => byId(id).snippets.find((s) => s.lang === lang)!;
    expect(label("openai", "python").label).toMatch(/openai/);
    expect(label("openai", "python").code).toContain("OpenAI(");
    expect(label("anthropic", "python").code).toContain("anthropic.Anthropic(");
    expect(label("openai", "javascript").label).toMatch(/fetch/);
    expect(label("openai", "curl").code.startsWith("curl")).toBe(true);
  });
  it("writes JavaScript that parses and a client configuration that is valid JSON", () => {
    for (const token of [false, true]) {
      for (const e of buildEndpoints(BASE, { token })) {
        for (const s of e.snippets) {
          if (s.lang === "javascript") expect(() => new AsyncFunction(s.code), `${e.id} javascript`).not.toThrow();
          if (s.lang === "config") expect(() => JSON.parse(s.code), `${e.id} config`).not.toThrow();
        }
      }
    }
  });

  describe("with the pairing token", () => {
    const withToken = buildEndpoints(BASE, { token: true });
    const code = (list: typeof eps, id: string, lang: string) => byId(id, list).snippets.find((s) => s.lang === lang)!.code;
    it("adds the header each protocol expects", () => {
      expect(code(withToken, "openai", "curl")).toContain(`Authorization: Bearer ${TOKEN_PLACEHOLDER}`);
      expect(code(withToken, "openai", "python")).toContain(`api_key="${TOKEN_PLACEHOLDER}"`);
      expect(code(withToken, "openai", "javascript")).toContain(`Authorization: "Bearer ${TOKEN_PLACEHOLDER}"`);
      expect(code(withToken, "anthropic", "curl")).toContain(`x-api-key: ${TOKEN_PLACEHOLDER}`);
      expect(code(withToken, "anthropic", "javascript")).toContain(`"x-api-key": "${TOKEN_PLACEHOLDER}"`);
      expect(JSON.parse(code(withToken, "mcp", "config")).mcpServers["ai-workspace"].headers.Authorization).toBe(`Bearer ${TOKEN_PLACEHOLDER}`);
      expect(code(withToken, "a2a", "python")).toContain(TOKEN_PLACEHOLDER);
    });
    it("leaves the examples without it by default, and the webhooks always (their secret is the credential)", () => {
      for (const e of eps) for (const s of e.snippets) expect(s.code).not.toContain(TOKEN_PLACEHOLDER);
      for (const s of byId("webhook", withToken).snippets) expect(s.code).not.toContain(TOKEN_PLACEHOLDER);
      expect(byId("webhook").guarded).toBe(false);
      expect(eps.filter((e) => e.guarded).map((e) => e.id)).toEqual(["openai", "anthropic", "mcp", "a2a"]);
    });
  });
});

describe("lanBaseUrl", () => {
  it("uses the LAN address of this device when the server is reachable, a placeholder otherwise", () => {
    expect(lanBaseUrl("http://192.168.1.10:7777/")).toBe("http://192.168.1.10:7777");
    expect(lanBaseUrl(null)).toBe("http://<ip-del-pc>:7777");
    expect(lanBaseUrl(undefined, 8080)).toBe("http://<ip-del-pc>:8080");
  });
});
