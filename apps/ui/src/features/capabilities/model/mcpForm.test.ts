import { describe, expect, it } from "vitest";
import { emptyRow } from "../../../shared/lib/kvRows";
import type { McpServer } from "../../../shared/api/types";
import { draftFromServer, draftToPayload, emptyDraft, isHttpUrl, isSecretKey, normalizeName, serverTarget, splitCommandLine, validateDraft } from "./mcpForm";

const stdio: McpServer = {
  id: "m1", name: "filesystem", transport: "stdio", command: "npx", args: ["-y", "server-fs", "~/My Documents"],
  env: { LOG: "debug" }, enabled: true, status: "connected", tools: [],
};
const http: McpServer = {
  id: "m2", name: "github", transport: "http", url: "http://127.0.0.1:8931/mcp",
  headers: { Authorization: "Bearer abc" }, enabled: false, status: "unreachable", tools: [], error: "connessione rifiutata",
};

describe("draft <-> payload", () => {
  it("round-trips a stdio server without losing spaces inside an argument", () => {
    const payload = draftToPayload(draftFromServer(stdio));
    expect(payload).toEqual({ id: "m1", name: "filesystem", enabled: true, transport: "stdio", command: "npx", args: ["-y", "server-fs", "~/My Documents"], env: { LOG: "debug" } });
  });
  it("round-trips an http server and keeps the enabled flag", () => {
    expect(draftToPayload(draftFromServer(http))).toEqual({
      id: "m2", name: "github", enabled: false, transport: "http", url: "http://127.0.0.1:8931/mcp", headers: { Authorization: "Bearer abc" },
    });
  });
  it("sends only the fields of the chosen transport", () => {
    const d = { ...draftFromServer(stdio), transport: "http" as const, url: " http://x.test/mcp " };
    const p = draftToPayload(d);
    expect(p).toMatchObject({ transport: "http", url: "http://x.test/mcp" });
    expect(p).not.toHaveProperty("command");
    expect(p).not.toHaveProperty("args");
    expect(p).not.toHaveProperty("env");
  });
  it("sends the mask of an untouched secret back as it came, and what the user typed otherwise", () => {
    const masked: McpServer = { ...http, headers: { Authorization: "••••••", Accept: "application/json" }, env: undefined };
    const d = draftFromServer(masked);
    expect(draftToPayload(d)).toMatchObject({ headers: { Authorization: "••••••", Accept: "application/json" } });
    const changed = { ...d, headers: d.headers.map((r) => (r.key === "Authorization" ? { ...r, value: "Bearer nuovo" } : r)) };
    expect(draftToPayload(changed)).toMatchObject({ headers: { Authorization: "Bearer nuovo", Accept: "application/json" } });
  });
  it("omits empty env and headers, and the id of a new server", () => {
    const p = draftToPayload({ ...emptyDraft(), name: "a b", command: "node", env: [emptyRow("", "x")] });
    expect(p).toEqual({ name: "a_b", enabled: true, transport: "stdio", command: "node", args: [] });
  });
});

describe("sampling and OAuth client id", () => {
  it("sends the sampling choice only once it was set, and the client id only when filled", () => {
    expect(draftToPayload(draftFromServer(http))).not.toHaveProperty("sampling");
    expect(draftToPayload({ ...draftFromServer(http), sampling: true })).toMatchObject({ sampling: true });
    expect(draftToPayload({ ...draftFromServer({ ...http, sampling: true }), sampling: false })).toMatchObject({ sampling: false });
    expect(draftToPayload({ ...draftFromServer(http), oauthClientId: " abc " })).toMatchObject({ oauth_client_id: "abc" });
    expect(draftToPayload(draftFromServer(http))).not.toHaveProperty("oauth_client_id");
    expect(draftFromServer({ ...http, oauth_client_id: "x" }).oauthClientId).toBe("x");
  });
});

describe("validateDraft", () => {
  const ok = { ...emptyDraft(), name: "fs", command: "npx" };
  it("accepts a complete stdio server", () => {
    expect(validateDraft(ok, [])).toEqual({});
  });
  it("needs a name that no other server uses (the engine turns spaces into underscores)", () => {
    expect(validateDraft({ ...ok, name: "  " }, []).name).toBeDefined();
    expect(validateDraft(ok, ["FS"]).name).toMatch(/esiste/i);
    expect(validateDraft({ ...ok, name: "my fs" }, ["my_fs"]).name).toMatch(/esiste/i);
  });
  it("needs a command for stdio and a valid http(s) url for http", () => {
    expect(validateDraft({ ...ok, command: "" }, []).command).toBeDefined();
    const h = { ...emptyDraft(), name: "gh", transport: "http" as const };
    expect(validateDraft(h, []).url).toBeDefined();
    expect(validateDraft({ ...h, url: "ftp://x" }, []).url).toMatch(/http/);
    expect(validateDraft({ ...h, url: "nope" }, []).url).toBeDefined();
    expect(validateDraft({ ...h, url: "https://x.test/mcp" }, [])).toEqual({});
  });
  it("flags repeated variables and headers (headers ignore case)", () => {
    expect(validateDraft({ ...ok, env: [emptyRow("A", "1"), emptyRow("A", "2")] }, []).env).toMatch(/A/);
    const h = { ...emptyDraft(), name: "gh", transport: "http" as const, url: "http://x.test", headers: [emptyRow("Authorization", "a"), emptyRow("authorization", "b")] };
    expect(validateDraft(h, []).headers).toMatch(/authorization/);
  });
});

describe("masked secrets", () => {
  const masked: McpServer = { ...http, headers: { Authorization: "••••••" } };
  const ok = { ...draftFromServer(masked), name: "gh" };
  it("keeps an untouched mask valid", () => {
    expect(validateDraft(ok, [])).toEqual({});
  });
  it("refuses a mask that has no stored secret behind it (renamed row, or typed by hand)", () => {
    const renamed = { ...ok, headers: ok.headers.map((r) => ({ ...r, key: "X-Token" })) };
    expect(validateDraft(renamed, []).headers).toMatch(/X-Token/);
    const typed = { ...ok, headers: [...ok.headers, emptyRow("X-Api-Key", "••••••")] };
    expect(validateDraft(typed, []).headers).toMatch(/X-Api-Key/);
    const stdio2 = { ...emptyDraft(), name: "fs", command: "npx", env: [emptyRow("GITHUB_TOKEN", "••••••")] };
    expect(validateDraft(stdio2, []).env).toMatch(/GITHUB_TOKEN/);
  });
});

describe("isHttpUrl", () => {
  it("accepts only http and https addresses with a host", () => {
    expect(isHttpUrl("http://127.0.0.1:8931/mcp")).toBe(true);
    expect(isHttpUrl("https://x.test")).toBe(true);
    for (const u of ["ftp://x.test", "x.test", "http://", "file:///etc/passwd", ""]) expect(isHttpUrl(u), u).toBe(false);
  });
});

describe("isSecretKey", () => {
  it("masks credentials by name (the backend's list: auth, cookie, token, key, secret, password, passwd, credential, signature)", () => {
    for (const k of ["Authorization", "authorization", "X-Api-Key", "GITHUB_TOKEN", "OPENAI_API_KEY", "client_secret", "DB_PASSWORD", "Cookie", "x-signature", " Set-Cookie "]) expect(isSecretKey(k)).toBe(true);
    for (const k of ["LOG_LEVEL", "Content-Type", "PATH", "Accept"]) expect(isSecretKey(k)).toBe(false);
  });
});

describe("splitCommandLine", () => {
  it("offers to split a whole command line, keeping quoted arguments together", () => {
    expect(splitCommandLine({ command: 'npx -y "my pkg"', args: ["x"] })).toEqual({ command: "npx", args: ["-y", "my pkg", "x"] });
  });
  it("leaves a plain command alone", () => {
    expect(splitCommandLine({ command: "npx", args: [] })).toBeNull();
    expect(splitCommandLine({ command: '"C:\\Program Files\\node.exe"', args: [] })).toBeNull();
  });
});

describe("serverTarget", () => {
  it("summarises what runs or where it connects", () => {
    expect(serverTarget(stdio)).toBe('npx -y server-fs "~/My Documents"');
    expect(serverTarget(http)).toBe("http://127.0.0.1:8931/mcp");
    expect(serverTarget({ transport: "stdio" })).toBe("");
  });
  it("normalizeName matches the engine", () => {
    expect(normalizeName("  my server ")).toBe("my_server");
  });
});
