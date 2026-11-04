import { describe, expect, it } from "vitest";
import { ApiError } from "../api/http";
import { shouldRetry } from "./client";

describe("shouldRetry", () => {
  it("retries a network or server failure once", () => {
    expect(shouldRetry(0, new TypeError("Failed to fetch"))).toBe(true);
    expect(shouldRetry(0, new ApiError(500, "boom"))).toBe(true);
    expect(shouldRetry(0, new ApiError(502, "provider"))).toBe(true);
    expect(shouldRetry(1, new ApiError(500, "boom"))).toBe(false);
  });
  it("never retries a client error: 401 (pairing), 404 (gone) or 400 (wrong request)", () => {
    for (const status of [400, 401, 403, 404, 409, 422]) expect(shouldRetry(0, new ApiError(status, "x")), String(status)).toBe(false);
  });
});
