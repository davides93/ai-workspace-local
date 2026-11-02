import { describe, expect, it } from "vitest";
import { serviceBaseUrl } from "../../../shared/lib/baseUrl";
import { buildEndpoints } from "./snippets";

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

describe("buildEndpoints", () => {
  const eps = buildEndpoints("http://127.0.0.1:7777/");
  const byId = (id: string) => eps.find((e) => e.id === id)!;

  it("lists the five integrations with the documented paths", () => {
    expect(eps.map((e) => e.id)).toEqual(["openai", "anthropic", "mcp", "a2a", "webhook"]);
    expect(byId("openai").url).toBe("http://127.0.0.1:7777/v1");
    expect(byId("anthropic").url).toBe("http://127.0.0.1:7777/v1/messages");
    expect(byId("mcp").url).toBe("http://127.0.0.1:7777/mcp");
    expect(byId("a2a").url).toBe("http://127.0.0.1:7777/a2a");
    expect(byId("a2a").extra?.[0].url).toBe("http://127.0.0.1:7777/.well-known/agent-card.json");
    expect(byId("webhook").url).toBe("http://127.0.0.1:7777/hooks");
  });
  it("gives every endpoint a copyable snippet built on the base URL", () => {
    for (const e of eps) {
      expect(e.snippets.length).toBeGreaterThan(0);
      for (const s of e.snippets) {
        expect(s.code).toContain("http://127.0.0.1:7777");
        expect(s.code).not.toContain("7777//");
      }
    }
    expect(byId("openai").snippets.some((s) => s.code.startsWith("curl"))).toBe(true);
    expect(byId("openai").snippets.some((s) => s.code.includes("OpenAI("))).toBe(true);
    expect(byId("anthropic").snippets.some((s) => s.code.includes("anthropic.Anthropic("))).toBe(true);
  });
});
