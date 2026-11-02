import { describe, expect, it } from "vitest";
import { authBody } from "../../../shared/api/http";
import { authPayload, buildUrlRequest, hasAuthChanges, hostOf, isHttpUrl, validateEndpoint, validateUrlForm } from "./forms";

const HASH = "a".repeat(64);

describe("direct URL form", () => {
  it("accepts only http(s) addresses", () => {
    expect(isHttpUrl("https://huggingface.co/x/resolve/main/m.gguf")).toBe(true);
    expect(isHttpUrl(" http://192.168.1.5:8080/m.gguf ")).toBe(true);
    expect(isHttpUrl("ftp://x.org/m.gguf")).toBe(false);
    expect(isHttpUrl("file:///etc/passwd")).toBe(false);
    expect(isHttpUrl("javascript:alert(1)")).toBe(false);
    expect(isHttpUrl("not a url")).toBe(false);
  });

  it("reports what is wrong in Italian and nothing for a valid form", () => {
    expect(validateUrlForm({ url: "", fileName: "", sha256: "" }).url).toMatch(/indirizzo/);
    expect(validateUrlForm({ url: "x.org/a.gguf", fileName: "", sha256: "" }).url).toMatch(/http/);
    expect(validateUrlForm({ url: "https://x.org/a.gguf", fileName: "", sha256: "" })).toEqual({});
    expect(validateUrlForm({ url: "https://x.org/a.gguf", fileName: "a.gguf", sha256: HASH })).toEqual({});
  });

  it("rejects a file name with folders and a malformed sha256", () => {
    for (const bad of ["../a.gguf", "dir/a.gguf", "a\\b.gguf", "..", "."]) {
      expect(validateUrlForm({ url: "https://x.org/a", fileName: bad, sha256: "" }).fileName).toBeTruthy();
    }
    expect(validateUrlForm({ url: "https://x.org/a", fileName: "", sha256: "xyz" }).sha256).toBeTruthy();
    expect(validateUrlForm({ url: "https://x.org/a", fileName: "", sha256: "a".repeat(63) }).sha256).toBeTruthy();
    expect(validateUrlForm({ url: "https://x.org/a", fileName: "", sha256: HASH.toUpperCase() })).toEqual({});
  });

  it("builds {url, file_name?, sha256?} leaving empty optionals out and lowercasing the hash", () => {
    expect(buildUrlRequest({ url: " https://x.org/a.gguf ", fileName: " ", sha256: "" })).toEqual({ url: "https://x.org/a.gguf" });
    expect(buildUrlRequest({ url: "https://x.org/a.gguf", fileName: "m.gguf", sha256: HASH.toUpperCase() })).toEqual({ url: "https://x.org/a.gguf", file_name: "m.gguf", sha256: HASH });
  });
});

describe("provider settings form", () => {
  const current = { endpoint: "https://huggingface.co" };

  it("an empty token box leaves the stored token alone", () => {
    expect(authPayload({ token: "", endpoint: current.endpoint, resetEndpoint: false }, current)).toEqual({ token: undefined, endpoint: undefined });
    expect(authPayload({ token: "   ", endpoint: current.endpoint, resetEndpoint: false }, current)).toEqual({ token: undefined, endpoint: undefined });
  });

  it("sends the new token trimmed and only the endpoint that changed", () => {
    expect(authPayload({ token: " hf_abc ", endpoint: current.endpoint, resetEndpoint: false }, current)).toEqual({ token: "hf_abc", endpoint: undefined });
    expect(authPayload({ token: "", endpoint: " https://hf-mirror.com ", resetEndpoint: false }, current)).toEqual({ token: undefined, endpoint: "https://hf-mirror.com" });
  });

  it("'ripristina' sends the empty string, which restores the default", () => {
    expect(authPayload({ token: "", endpoint: "https://hf-mirror.com", resetEndpoint: true }, current)).toEqual({ token: undefined, endpoint: "" });
  });

  it("tells whether there is anything to save", () => {
    expect(hasAuthChanges({ token: undefined, endpoint: undefined })).toBe(false);
    expect(hasAuthChanges({ token: "t", endpoint: undefined })).toBe(true);
    expect(hasAuthChanges({ token: undefined, endpoint: "" })).toBe(true);
  });

  it("allows an empty endpoint (the default) but not a non-http one", () => {
    expect(validateEndpoint("")).toBeNull();
    expect(validateEndpoint("https://hf-mirror.com")).toBeNull();
    expect(validateEndpoint("hf-mirror.com")).toBeTruthy();
    expect(hostOf("https://huggingface.co/x")).toBe("huggingface.co");
    expect(hostOf("")).toBeNull();
  });
});

describe("body of PUT …/auth", () => {
  it("leaves out a field that is not being changed", () => {
    expect(authBody(undefined, "https://m")).toEqual({ endpoint: "https://m" });
    expect(JSON.stringify(authBody(undefined, undefined))).toBe("{}");
    expect(authBody("tok")).toEqual({ token: "tok" });
  });

  it("sends token null to remove it and endpoint '' to restore the default", () => {
    expect(JSON.stringify(authBody(null, ""))).toBe('{"token":null,"endpoint":""}');
  });
});
