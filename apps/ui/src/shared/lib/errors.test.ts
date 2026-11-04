import { describe, expect, it } from "vitest";
import { ApiError } from "../api/http";
import { errorMessage, errorStatus, ignoreNotFound, isNotFound, isUnauthorized } from "./errors";

describe("errors", () => {
  it("reads the message and the HTTP status of whatever was thrown", () => {
    expect(errorMessage(new ApiError(404, "sessione x non trovata"))).toBe("sessione x non trovata");
    expect(errorMessage("testo")).toBe("testo");
    expect(errorMessage({ message: "m" })).toBe("m");
    expect(errorMessage(null)).toBe("Errore sconosciuto");
    expect(errorStatus(new ApiError(409, "x"))).toBe(409);
    expect(errorStatus(new Error("x"))).toBeUndefined();
  });
  it("tells a 404 (gone) and a 401 (pairing) apart", () => {
    expect(isNotFound(new ApiError(404, "x"))).toBe(true);
    expect(isNotFound(new ApiError(400, "x"))).toBe(false);
    expect(isNotFound(new Error("404"))).toBe(false);
    expect(isUnauthorized(new ApiError(401, "x"))).toBe(true);
  });
  it("treats deleting something that is already gone as done, and rethrows anything else", async () => {
    await expect(ignoreNotFound(Promise.reject(new ApiError(404, "gone")))).resolves.toBeUndefined();
    await expect(ignoreNotFound(Promise.resolve("ok"))).resolves.toBeUndefined();
    await expect(ignoreNotFound(Promise.reject(new ApiError(409, "in uso")))).rejects.toMatchObject({ status: 409 });
    await expect(ignoreNotFound(Promise.reject(new Error("rete")))).rejects.toThrow("rete");
  });
});
