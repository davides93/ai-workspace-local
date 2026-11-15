import { describe, expect, it } from "vitest";
import { isOutsideWorkspace } from "./folderHint";

describe("isOutsideWorkspace", () => {
  it("recognises the refusals of the file tools, with or without the «Negato» prefix", () => {
    expect(isOutsideWorkspace("/Users/anna/Downloads è fuori dal workspace (/Users/anna/AIW-Workspace)")).toBe(true);
    expect(isOutsideWorkspace("Negato: /Users/anna/Downloads è fuori dal workspace (/x). Se serve lavorare lì, solo l'utente può…")).toBe(true);
    expect(isOutsideWorkspace("ERRORE: /x/link punta fuori dal workspace tramite un collegamento simbolico (/y)")).toBe(true);
  });
  it("ignores anything else", () => {
    expect(isOutsideWorkspace(undefined)).toBe(false);
    expect(isOutsideWorkspace("")).toBe(false);
    expect(isOutsideWorkspace("Negato: sandbox in sola lettura: le scritture non sono consentite")).toBe(false);
    expect(isOutsideWorkspace("Negato: /x è nascosto agli agenti: contiene i dati dell'app o delle credenziali")).toBe(false);
    expect(isOutsideWorkspace("Creato notes/a.md")).toBe(false);
  });
});
