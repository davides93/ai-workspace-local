import { describe, expect, it } from "vitest";
import type { KbCollection, KbJob } from "../../../shared/api/types";
import { coverageText, jobPercent, jobSummary, pickCollection, runningJobs, splitBatches } from "./kb";

const f = (name: string, size: number) => ({ name, size });
const job = (over: Partial<KbJob> = {}): KbJob => ({ id: "j", collection_id: "c", state: "running", total: 4, done: 1, added: 0, unchanged: 0, replaced: 0, failed: 0, skipped: [], skipped_total: 0, ...over });
const col = (over: Partial<KbCollection> = {}): KbCollection => ({ id: "c", name: "Casa", description: "", created_at: "", documents: 1, chunks: 10, embedded_chunks: 0, ...over });

describe("splitBatches", () => {
  it("groups at most 8 files per call", () => {
    const files = Array.from({ length: 19 }, (_, i) => f(`a${i}.txt`, 10));
    expect(splitBatches(files).batches.map((b) => b.length)).toEqual([8, 8, 3]);
  });
  it("starts a new group when the 25 MB total would be passed", () => {
    const mb = 1024 * 1024;
    const files = [f("a", 9 * mb), f("b", 9 * mb), f("c", 9 * mb)];
    expect(splitBatches(files).batches.map((b) => b.length)).toEqual([2, 1]);
  });
  it("leaves out empty and too big files and says why", () => {
    const r = splitBatches([f("vuoto.txt", 0), f("enorme.pdf", 11 * 1024 * 1024), f("ok.txt", 5)]);
    expect(r.batches).toEqual([[f("ok.txt", 5)]]);
    expect(r.errors).toHaveLength(2);
    expect(r.errors[0]).toContain("vuoto.txt");
    expect(r.errors[1]).toContain("enorme.pdf");
  });
  it("sends nothing for no files", () => {
    expect(splitBatches([])).toEqual({ batches: [], errors: [] });
  });
});

describe("jobs", () => {
  it("computes the percentage", () => {
    expect(jobPercent(job())).toBe(25);
    expect(jobPercent(job({ total: 0, done: 0 }))).toBe(0);
    expect(jobPercent(job({ state: "done", done: 4 }))).toBe(100);
  });
  it("summarises the outcome", () => {
    expect(jobSummary(job({ state: "done", added: 2, unchanged: 1, skipped_total: 3 }))).toBe("2 documenti aggiunti, 1 già presente, 3 file saltati.");
    expect(jobSummary(job({ state: "done", added: 1, replaced: 1, failed: 1 }))).toBe("1 documento aggiunto, 1 aggiornato, 1 non riuscito.");
    expect(jobSummary(job({ state: "done" }))).toBe("Nessuna novità.");
    expect(jobSummary(job({ state: "cancelled", added: 1 }))).toBe("Interrotto: 1 documento aggiunto.");
  });
  it("lists the running jobs of a collection", () => {
    const status = { jobs: [job({ id: "1" }), job({ id: "2", collection_id: "d" }), job({ id: "3", state: "done" })] };
    expect(runningJobs(status).map((j) => j.id)).toEqual(["1", "2"]);
    expect(runningJobs(status, "d").map((j) => j.id)).toEqual(["2"]);
    expect(runningJobs(undefined)).toEqual([]);
  });
});

describe("collections", () => {
  it("says how much can be searched by meaning", () => {
    expect(coverageText(col({ chunks: 0 }))).toBe("");
    expect(coverageText(col())).toBe("Ricerca per parole chiave");
    expect(coverageText(col({ embedded_chunks: 4 }))).toBe("Ricerca per significato pronta su 4 passaggi su 10");
    expect(coverageText(col({ embedded_chunks: 10 }))).toBe("Ricerca anche per significato");
  });
  it("keeps the chosen collection while it exists", () => {
    const list = [col({ id: "a" }), col({ id: "b" })];
    expect(pickCollection(list, "b")).toBe("b");
    expect(pickCollection(list, "zzz")).toBe("a");
    expect(pickCollection([], "a")).toBeNull();
    expect(pickCollection(undefined, null)).toBeNull();
  });
});
