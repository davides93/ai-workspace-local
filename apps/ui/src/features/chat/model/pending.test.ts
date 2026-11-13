import { describe, expect, it } from "vitest";
import { localAttachments, toPending, toUploads } from "./pending";

const file = (name: string, type: string, content = "ciao") => new File([content], name, { type });

describe("toPending", () => {
  it("describes a chosen file", () => {
    const p = toPending(file("dati.csv", "text/csv"));
    expect(p).toMatchObject({ name: "dati.csv", mime: "text/csv", size: 4, kind: "text" });
    expect(toPending(file("a.pdf", "application/pdf")).kind).toBe("document");
    expect(toPending(file("a.bin", "")).kind).toBe("file");
  });
  it("gives a pasted picture a name of its own, but never renames a real file", () => {
    expect(toPending(file("image.png", "image/png"), true).name).toMatch(/^Immagine \d\d-\d\d-\d\d\.png$/);
    expect(toPending(file("schermata.png", "image/png"), true).name).toBe("schermata.png");
    expect(toPending(file("image.png", "image/png"), false).name).toBe("image.png");
  });
  it("keys differ", () => {
    expect(toPending(file("a", "")).key).not.toBe(toPending(file("a", "")).key);
  });
});

describe("toUploads / localAttachments", () => {
  it("encodes the bytes as base64 and shows them to the row of the message just sent", async () => {
    const files = [toPending(file("a.txt", "text/plain", "ciao")), toPending(file("b.png", "image/png", "RIFF"))];
    const uploads = await toUploads(files);
    expect(uploads).toEqual([{ name: "a.txt", mime: "text/plain", data: "Y2lhbw==" }, { name: "b.png", mime: "image/png", data: "UklGRg==" }]);
    expect(localAttachments(files, uploads)).toMatchObject([
      { id: files[0].key, name: "a.txt", size: 4, kind: "text", data: "Y2lhbw==" },
      { id: files[1].key, name: "b.png", kind: "image", data: "UklGRg==" },
    ]);
  });
});
