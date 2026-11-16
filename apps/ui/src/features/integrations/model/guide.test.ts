import { describe, expect, it } from "vitest";
import { buildClientGuides } from "./guide";
import { buildEndpoints, TOKEN_PLACEHOLDER } from "./snippets";

const BASE = "http://192.168.1.20:7777";

describe("buildClientGuides", () => {
  const local = buildClientGuides(`${BASE}/`);
  const lan = buildClientGuides(BASE, { token: true });
  const code = (list: typeof local, id: string) => list.find((g) => g.id === id)!.code;

  it("covers the two projects of CopilotKit, with a note each", () => {
    expect(local.map((g) => g.id)).toEqual(["openbot", "opendots"]);
    for (const g of local) expect(g.note.length).toBeGreaterThan(20);
  });
  it("points the model at /v1 and, for OpenBot, asks for Chat Completions (there is no Responses API)", () => {
    expect(code(local, "openbot")).toContain(`OPENAI_BASE_URL=${BASE}/v1\n`);
    expect(code(local, "openbot")).toContain("COPILOTKIT_OPENAI_API=chat");
    expect(code(local, "opendots")).toContain(`OPENAI_BASE_URL=${BASE}/v1\n`);
    expect(code(local, "opendots")).not.toContain("COPILOTKIT_OPENAI_API=responses");
  });
  it("lists the agent address as host:port, never as a URL (OpenBot stops at startup on an entry written as a URL)", () => {
    const line = code(local, "openbot").split("\n").find((l) => l.startsWith("AGENT_ENDPOINT_ALLOWED_HOSTS="))!;
    expect(line).toBe("AGENT_ENDPOINT_ALLOWED_HOSTS=192.168.1.20:7777");
    expect(line).not.toMatch(/https?:\/\/|\*/);
    expect(code(local, "openbot")).toContain(`${BASE}/agui/assistant`);
  });
  it("puts the token where the project reads it, only for a client on another device", () => {
    expect(code(lan, "openbot")).toContain(`OPENAI_API_KEY=${TOKEN_PLACEHOLDER}`);
    expect(code(lan, "openbot")).toContain(`Authorization: Bearer ${TOKEN_PLACEHOLDER}`);
    expect(code(lan, "opendots")).toContain(`OPENAI_API_KEY=${TOKEN_PLACEHOLDER}`);
    for (const g of local) expect(g.code).not.toContain(TOKEN_PLACEHOLDER);
  });
});

describe("the AG-UI endpoint", () => {
  const agui = (opts = {}) => buildEndpoints(BASE, opts).find((e) => e.id === "agui")!;
  it("is the agent route, with curl, @ag-ui/client and the CopilotKit runtime", () => {
    expect(agui().url).toBe(`${BASE}/agui/assistant`);
    expect(agui().guarded).toBe(true);
    expect(agui().snippets.map((s) => s.lang)).toEqual(["curl", "javascript", "typescript"]);
    const ts = agui().snippets.find((s) => s.lang === "typescript")!.code;
    expect(ts).toContain('from "@copilotkit/runtime/v2"');
    expect(ts).toContain('import { HttpAgent } from "@ag-ui/client"');
    expect(ts).toContain("agents: {");
  });
  it("sends the token as a header of the agent for a client on another device, and not otherwise", () => {
    for (const s of agui({ token: true }).snippets) expect(s.code).toContain(`Bearer ${TOKEN_PLACEHOLDER}`);
    for (const s of agui().snippets) expect(s.code).not.toContain(TOKEN_PLACEHOLDER);
  });
});
