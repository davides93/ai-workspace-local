import { describe, expect, it } from "vitest";
import { addWithLimits, guessKind, humanSize, MAX_ATTACHMENTS, MAX_ATTACHMENT_BYTES, pastedName } from "./attachments";

describe("humanSize", () => {
  it("speaks in bytes, KB and MB with a decimal comma", () => {
    expect(humanSize(0)).toBe("0 byte");
    expect(humanSize(1023)).toBe("1023 byte");
    expect(humanSize(1536)).toBe("1,5 KB");
    expect(humanSize(20 * 1024)).toBe("20 KB");
    expect(humanSize(1.5 * 1024 * 1024)).toBe("1,5 MB");
    expect(humanSize(10 * 1024 * 1024)).toBe("10,0 MB");
  });
});

describe("guessKind", () => {
  it("tells images, documents, text and the rest", () => {
    expect(guessKind("foto.png", "image/png")).toBe("image");
    expect(guessKind("logo.svg", "image/svg+xml")).toBe("file");
    expect(guessKind("scheda.pdf", "")).toBe("document");
    expect(guessKind("note.md", "")).toBe("text");
    expect(guessKind("dati", "text/csv")).toBe("text");
    expect(guessKind("main.rs", "application/octet-stream")).toBe("text");
    expect(guessKind("archivio.zip", "application/zip")).toBe("file");
  });
});

describe("pastedName", () => {
  it("names a pasted image after the time", () => {
    expect(pastedName("image/png", new Date(2026, 0, 2, 9, 5, 7))).toBe("Immagine 09-05-07.png");
    expect(pastedName("image/jpeg", new Date(2026, 0, 2, 14, 32, 5))).toBe("Immagine 14-32-05.jpg");
  });
});

describe("addWithLimits", () => {
  const f = (name: string, size: number) => ({ name, size });
  it("keeps what fits and explains the rest in Italian", () => {
    const r = addWithLimits([f("a.txt", 10)], [f("b.txt", 20), f("grande.bin", MAX_ATTACHMENT_BYTES + 1), f("vuoto.txt", 0)]);
    expect(r.files.map((x) => x.name)).toEqual(["a.txt", "b.txt"]);
    expect(r.errors).toHaveLength(2);
    expect(r.errors[0]).toContain("«grande.bin» è troppo grande");
    expect(r.errors[1]).toContain("è vuoto");
  });
  it("stops at the count and at the total size", () => {
    const many = Array.from({ length: MAX_ATTACHMENTS + 2 }, (_, i) => f(`${i}.txt`, 1));
    const r = addWithLimits([], many);
    expect(r.files).toHaveLength(MAX_ATTACHMENTS);
    expect(r.errors).toHaveLength(2);
    expect(r.errors[0]).toContain("al massimo 8 file");
    const big = addWithLimits([], [f("1", 9 * 1024 * 1024), f("2", 9 * 1024 * 1024), f("3", 9 * 1024 * 1024)]);
    expect(big.files).toHaveLength(2);
    expect(big.errors[0]).toContain("in totale");
  });
});
