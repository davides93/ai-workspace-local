import { describe, expect, it } from "vitest";
import { MockTransport } from "./index";
import { errorStatus } from "../../lib/errors";

// The demo carrier must follow the server's semantics for the provider settings the dialog relies on
// (docs/architecture/model-hub.md §9) and reproduce the failures the models UI has to handle.

describe("provider settings on the demo transport", () => {
  it("token absent keeps the stored token, null removes it", async () => {
    const api = new MockTransport();
    expect((await api.setProviderAuth("hf", "hf_secret")).auth_configured).toBe(true);
    const changed = await api.setProviderAuth("hf", undefined, "https://hf-mirror.example");
    expect(changed.auth_configured).toBe(true);
    expect(changed.endpoint).toBe("https://hf-mirror.example");
    expect((await api.setProviderAuth("hf", null)).auth_configured).toBe(false);
  });

  it("endpoint absent keeps it, empty string restores the default", async () => {
    const api = new MockTransport();
    await api.setProviderAuth("hf", undefined, "https://hf-mirror.example");
    expect((await api.setProviderAuth("hf", "tok")).endpoint).toBe("https://hf-mirror.example");
    expect((await api.setProviderAuth("hf", undefined, "")).endpoint).toBe("https://huggingface.co");
  });

  it("Ollama takes an endpoint but no token", async () => {
    const api = new MockTransport();
    expect((await api.setProviderAuth("ollama", undefined, "https://ollama-mirror.example")).endpoint).toBe("https://ollama-mirror.example");
    await expect(api.setProviderAuth("ollama", "x")).rejects.toThrow();
  });
});

describe("failures of the demo hub", () => {
  it("a gated repository answers 401 until a token is set", async () => {
    const api = new MockTransport();
    const err = await api.remoteModel("hf", "meta-llama/Llama-3.2-3B-Instruct").catch((e: unknown) => e);
    expect(errorStatus(err)).toBe(401);
    await api.setProviderAuth("hf", "hf_secret");
    expect((await api.remoteModel("hf", "meta-llama/Llama-3.2-3B-Instruct")).variants.length).toBeGreaterThan(0);
  });

  it("searching for 'offline' simulates an unreachable provider (502)", async () => {
    const err = await new MockTransport().searchModels({ provider: "modelscope", q: "offline" }).catch((e: unknown) => e);
    expect(errorStatus(err)).toBe(502);
  });

  it("the curated catalog has the MLX entry, downloadable as a whole repository but not on this PC", async () => {
    const mlx = (await new MockTransport().catalog()).find((c) => c.format === "mlx");
    expect(mlx).toBeDefined();
    expect(mlx!.downloadable).toBe(true);
    expect(mlx!.source).toEqual({ kind: "hf", repo: expect.any(String), file: "" });
    expect(mlx!.fit).toBe("no");
  });

  it("the installed list covers every origin and the AIW models have a path", async () => {
    const models = await new MockTransport().models();
    expect(new Set(models.map((m) => m.origin))).toEqual(new Set(["aiw", "manual", "ollama", "lmstudio", "hf-cache"]));
    expect(models.filter((m) => m.origin === "aiw").every((m) => !!m.path)).toBe(true);
    expect(models.filter((m) => !m.removable).every((m) => ["ollama", "lmstudio", "hf-cache"].includes(m.origin))).toBe(true);
  });
});
