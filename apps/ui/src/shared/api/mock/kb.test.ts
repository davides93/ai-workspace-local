import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockTransport } from "./index";

const b64 = (text: string) => btoa(text);
const file = (name: string, text: string, mime = "text/plain") => ({ name, mime, data: b64(text) });

describe("knowledge base in the demo", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("starts with a demo collection that can be searched by keywords", async () => {
    const t = new MockTransport();
    const cols = await t.kbCollections();
    expect(cols.some((c) => c.name === "Casa" && c.documents > 0 && c.chunks > 0)).toBe(true);
    const r = await t.kbSearch({ query: "canone affitto" });
    expect(r.semantic).toBe(false);
    expect(r.hits[0].document_name).toBe("contratto-affitto.txt");
    expect((await t.kbStatus()).embedding_model).toBeUndefined();
  });

  it("validates collections like the engine", async () => {
    const t = new MockTransport();
    await expect(t.createKbCollection({ name: "   " })).rejects.toMatchObject({ status: 400, message: "Dai un nome alla raccolta." });
    const c = await t.createKbCollection({ name: "Lavoro", description: "Note" });
    await expect(t.createKbCollection({ name: "lavoro" })).rejects.toMatchObject({ status: 409 });
    const u = await t.updateKbCollection(c.id, { name: "Ufficio" });
    expect(u.name).toBe("Ufficio");
    await expect(t.kbDocuments("nope")).rejects.toMatchObject({ status: 404 });
    await t.deleteKbCollection(c.id);
    expect((await t.kbCollections()).some((x) => x.id === c.id)).toBe(false);
  });

  it("adds files in the background, skips what it cannot read and skips duplicates", async () => {
    const t = new MockTransport();
    const c = await t.createKbCollection({ name: "Prova" });
    const { job } = await t.addKbFiles(c.id, [file("a.txt", "Il gatto dorme sul divano."), file("b.png", "x", "image/png"), file("vuoto.txt", "")]);
    expect(job.state).toBe("running");
    expect(job.skipped_total).toBe(2);
    expect((await t.kbStatus()).jobs[0].state).toBe("running");
    await vi.advanceTimersByTimeAsync(800);
    const done = (await t.kbStatus()).jobs[0];
    expect(done).toMatchObject({ state: "done", added: 1, done: 1 });
    expect((await t.kbDocuments(c.id)).map((d) => d.name)).toEqual(["a.txt"]);
    expect((await t.kbSearch({ query: "gatto", collection: "Prova" })).hits).toHaveLength(1);

    await t.addKbFiles(c.id, [file("a.txt", "Il gatto dorme sul divano.")]);
    await vi.advanceTimersByTimeAsync(800);
    expect((await t.kbStatus()).jobs[0]).toMatchObject({ added: 0, unchanged: 1 });
  });

  it("refuses too many or too big files with the engine's messages", async () => {
    const t = new MockTransport();
    const c = (await t.kbCollections())[0];
    const many = Array.from({ length: 9 }, (_, i) => file(`f${i}.txt`, "x"));
    await expect(t.addKbFiles(c.id, many)).rejects.toMatchObject({ status: 400, message: "Puoi aggiungere al massimo 8 file per volta." });
    await expect(t.addKbFiles(c.id, [])).rejects.toMatchObject({ status: 400, message: "Scegli almeno un file." });
    const big = { name: "grande.txt", mime: "text/plain", data: "A".repeat(15 * 1024 * 1024) };
    await expect(t.addKbFiles(c.id, [big])).rejects.toMatchObject({ status: 400 });
  });

  it("simulates a folder, reindexes and cancels", async () => {
    const t = new MockTransport();
    const c = await t.createKbCollection({ name: "Cartella" });
    await expect(t.addKbPath(c.id, "relativo/dir")).rejects.toMatchObject({ status: 400 });
    const { job } = await t.addKbPath(c.id, "/home/g/Documenti");
    await t.cancelKbJob(job.id);
    expect((await t.kbStatus()).jobs[0].state).toBe("cancelled");
    await expect(t.cancelKbJob(job.id)).rejects.toMatchObject({ status: 404 });
    await t.addKbPath(c.id, "/home/g/Documenti");
    await vi.advanceTimersByTimeAsync(800);
    expect((await t.kbDocuments(c.id)).length).toBeGreaterThan(0);
    const r = await t.reindexKbCollection(c.id);
    expect(r.job.total).toBeGreaterThan(0);
  });

  it("deletes documents and validates searches", async () => {
    const t = new MockTransport();
    const c = (await t.kbCollections())[0];
    const docs = await t.kbDocuments(c.id);
    await t.deleteKbDocument(docs[0].id);
    expect((await t.kbDocuments(c.id)).length).toBe(docs.length - 1);
    await expect(t.deleteKbDocument("zzz")).rejects.toMatchObject({ status: 404 });
    await expect(t.kbSearch({ query: "  " })).rejects.toMatchObject({ message: "Scrivi cosa cercare." });
    await expect(t.kbSearch({ query: "x", collection: "inesistente" })).rejects.toMatchObject({ status: 404 });
  });

  it("checks the collections an agent points at", async () => {
    const t = new MockTransport();
    const agent = (await t.agents())[0];
    await expect(t.saveAgent({ ...agent, knowledge: ["kbk_nope"] })).rejects.toMatchObject({ status: 400, message: "documenti: la raccolta «kbk_nope» non esiste" });
    const saved = await t.saveAgent({ ...agent, knowledge: ["kbk_demo_casa"] });
    expect(saved.knowledge).toEqual(["kbk_demo_casa"]);
  });
});
