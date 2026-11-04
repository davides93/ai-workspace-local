import { describe, expect, it } from "vitest";
import { errorHint, errorTitle, needsToken, shouldRetryHub } from "./errors";

const http = (status: number, message = "boom") => Object.assign(new Error(message), { status });

describe("needsToken", () => {
  it("is true for 401 and 403 (token missing, refused or without access)", () => {
    expect(needsToken(http(401))).toBe(true);
    expect(needsToken(http(403))).toBe(true);
  });
  it("is true for a job message that says the repository is gated or asks for a token", () => {
    expect(needsToken(undefined, "repository gated: serve un token Hugging Face")).toBe(true);
    expect(needsToken(new Error("serve un token"))).toBe(true);
  });
  it("is false for the other failures", () => {
    expect(needsToken(http(502, "Provider irraggiungibile"))).toBe(false);
    expect(needsToken(http(404, "non trovato"))).toBe(false);
    expect(needsToken(undefined, "spazio insufficiente")).toBe(false);
    expect(needsToken(undefined)).toBe(false);
  });
});

describe("hints and titles by HTTP status", () => {
  it("gives the next step for the statuses of the hub", () => {
    expect(errorHint(http(401))).toMatch(/token/);
    expect(errorHint(http(502))).toMatch(/endpoint/);
    expect(errorHint(http(507))).toMatch(/spazio/);
    expect(errorHint(http(409))).toMatch(/già in scrittura/);
    expect(errorHint(http(404))).toMatch(/nome/);
    expect(errorHint(http(500))).toBeNull();
    expect(errorHint(new Error("plain"))).toBeNull();
  });

  it("titles a block error by what went wrong", () => {
    expect(errorTitle(http(401))).toBe("Accesso negato dal provider");
    expect(errorTitle(http(404))).toBe("Non trovato");
    expect(errorTitle(http(502))).toBe("Provider irraggiungibile");
    expect(errorTitle(http(507))).toBe("Spazio su disco insufficiente");
    expect(errorTitle(new Error("x"))).toBe("Qualcosa non ha funzionato");
  });
});

describe("shouldRetryHub", () => {
  it("retries a network or server error once", () => {
    expect(shouldRetryHub(0, http(502))).toBe(true);
    expect(shouldRetryHub(0, new Error("Failed to fetch"))).toBe(true);
    expect(shouldRetryHub(1, http(502))).toBe(false);
  });
  it("never retries a 4xx: asking again will not add a token or fix a name", () => {
    for (const s of [400, 401, 403, 404, 409]) expect(shouldRetryHub(0, http(s))).toBe(false);
  });
});
