import { describe, expect, it } from "vitest";
import { isSecretName, SECRET_MASK } from "./secrets";

describe("isSecretName", () => {
  it("recognises credentials by a fragment of the name, whatever the case (same rule as the backend)", () => {
    for (const n of [
      "Authorization", "authorization", "Proxy-Authorization", "Cookie", "Set-Cookie", "X-Api-Key", "X-Auth-Token",
      "GITHUB_PERSONAL_ACCESS_TOKEN", "OPENAI_API_KEY", "CLIENT_SECRET", "DB_PASSWORD", "DB_PASSWD", "x-signature", "AWS_CREDENTIALS",
    ]) expect(isSecretName(n), n).toBe(true);
  });
  it("leaves ordinary names alone", () => {
    for (const n of ["LOG_LEVEL", "Content-Type", "PATH", "Accept", "User-Agent", "HOME", ""]) expect(isSecretName(n), n).toBe(false);
  });
  it("exposes the mask the backend uses", () => {
    expect(SECRET_MASK).toBe("••••••");
  });
});
