import { describe, expect, it } from "vitest";
import type { AuditEntry } from "../../../shared/api/types";
import { ALL, auditExport, decisionInfo, emptyFilters, facets, filterAudit, isFiltering, newestFirst, paginate } from "./filter";

const row = (id: string, p: Partial<AuditEntry> = {}): AuditEntry => ({ id, ts: `2026-11-02T10:0${id}:00Z`, actor: "agent", action: "tool", detail: "", ...p });
const rows: AuditEntry[] = [
  row("1", { tool: "shell", decision: "approved", detail: "mkdir -p 2024 && mv *.pdf", run_id: "run_a" }),
  row("2", { tool: "read_file", decision: "allow", detail: "fatture/2025-03.pdf", run_id: "run_a" }),
  row("3", { actor: "user", action: "runtime.start", detail: "llama-server · qwen3.5" }),
  row("4", { tool: "write_file", decision: "deny", detail: "C:\\Windows\\hosts fuori dal workspace", run_id: "run_b" }),
  row("5", { actor: "system", action: "kill", detail: "Kill switch: 1 run fermato" }),
];

describe("filterAudit", () => {
  it("returns everything without filters", () => {
    expect(filterAudit(rows, emptyFilters)).toHaveLength(5);
    expect(isFiltering(emptyFilters)).toBe(false);
  });
  it("filters by actor, decision and tool", () => {
    expect(filterAudit(rows, { ...emptyFilters, actor: "user" }).map((r) => r.id)).toEqual(["3"]);
    expect(filterAudit(rows, { ...emptyFilters, decision: "deny" }).map((r) => r.id)).toEqual(["4"]);
    expect(filterAudit(rows, { ...emptyFilters, tool: "shell" }).map((r) => r.id)).toEqual(["1"]);
    expect(filterAudit(rows, { ...emptyFilters, actor: "agent", decision: "allow" }).map((r) => r.id)).toEqual(["2"]);
  });
  it("can pick the entries without a decision", () => {
    expect(filterAudit(rows, { ...emptyFilters, decision: "none" }).map((r) => r.id)).toEqual(["3", "5"]);
  });
  it("searches every column, ignoring case and accents", () => {
    expect(filterAudit(rows, { ...emptyFilters, q: "PDF" }).map((r) => r.id)).toEqual(["1", "2"]);
    expect(filterAudit(rows, { ...emptyFilters, q: "run_b" }).map((r) => r.id)).toEqual(["4"]);
    expect(filterAudit(rows, { ...emptyFilters, q: "kill switch" }).map((r) => r.id)).toEqual(["5"]);
    expect(filterAudit(rows, { ...emptyFilters, q: "tool shell" }).map((r) => r.id)).toEqual(["1"]);
  });
  it("combines the filters", () => {
    expect(filterAudit(rows, { actor: "agent", decision: ALL, tool: ALL, q: "run_a" })).toHaveLength(2);
    expect(filterAudit(rows, { actor: "user", decision: ALL, tool: ALL, q: "run_a" })).toHaveLength(0);
    expect(isFiltering({ ...emptyFilters, q: " x " })).toBe(true);
    expect(isFiltering({ ...emptyFilters, q: "  " })).toBe(false);
  });
});

describe("facets", () => {
  it("lists the decisions in a sensible order, the tools alphabetically, and tells about entries without decision", () => {
    expect(facets(rows)).toEqual({ decisions: ["allow", "approved", "deny"], tools: ["read_file", "shell", "write_file"], hasUndecided: true });
    expect(facets([rows[0]]).hasUndecided).toBe(false);
  });
  it("keeps an unknown decision after the known ones", () => {
    expect(facets([row("1", { decision: "boh" as AuditEntry["decision"] }), row("2", { decision: "deny" })]).decisions).toEqual(["deny", "boh"]);
  });
});

describe("paginate", () => {
  const many = Array.from({ length: 120 }, (_, i) => i);
  it("slices a page and reports the range", () => {
    const p = paginate(many, 2, 50);
    expect(p).toMatchObject({ page: 2, pages: 3, from: 51, to: 100, total: 120 });
    expect(p.rows[0]).toBe(50);
  });
  it("clamps the page to the ones that exist (the list shrank, or there is nothing)", () => {
    expect(paginate(many, 9, 50)).toMatchObject({ page: 3, from: 101, to: 120 });
    expect(paginate(many, 0, 50).page).toBe(1);
    expect(paginate([], 4, 50)).toMatchObject({ page: 1, pages: 1, from: 0, to: 0, total: 0, rows: [] });
  });
});

describe("export and labels", () => {
  it("names the file by local date and time and carries the rows with the filters", () => {
    const out = auditExport(rows.slice(0, 2), { ...emptyFilters, actor: "agent" }, new Date(2026, 10, 2, 9, 5, 0));
    expect(out.filename).toBe("attivita-20261102-0905.json");
    const json = JSON.parse(out.content);
    expect(json.count).toBe(2);
    expect(json.entries.map((e: AuditEntry) => e.id)).toEqual(["1", "2"]);
    expect(json.filters).toEqual({ actor: "agent", decision: "all", tool: "all", q: "" });
    expect(json.source).toContain("500");
  });
  it("leaves filters out of the export when none is set", () => {
    expect(JSON.parse(auditExport(rows, emptyFilters).content).filters).toBeNull();
  });
  it("sorts newest first", () => {
    expect(newestFirst(rows).map((r) => r.id)).toEqual(["5", "4", "3", "2", "1"]);
  });
  it("words the decisions in Italian", () => {
    expect(decisionInfo("deny")).toEqual({ label: "Negato", tone: "crit" });
    expect(decisionInfo("boh")).toEqual({ label: "boh", tone: "neutral" });
  });
});
