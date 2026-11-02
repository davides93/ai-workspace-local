import { describe, expect, it } from "vitest";
import { duplicateKeys, emptyRow, recordToRows, rowsToRecord } from "./kvRows";

describe("kvRows", () => {
  it("round-trips a record and keeps the order", () => {
    const rec = { B: "2", A: "1" };
    expect(Object.keys(rowsToRecord(recordToRows(rec)))).toEqual(["B", "A"]);
    expect(rowsToRecord(recordToRows(rec))).toEqual(rec);
  });
  it("gives every row its own id", () => {
    const rows = recordToRows({ a: "1", b: "2" });
    expect(new Set(rows.map((r) => r.id)).size).toBe(2);
    expect(emptyRow().id).not.toBe(emptyRow().id);
  });
  it("drops unnamed rows, trims names and keeps the last of a repeated name", () => {
    expect(rowsToRecord([emptyRow("  ", "x"), emptyRow(" A ", "1"), emptyRow("A", "2")])).toEqual({ A: "2" });
  });
  it("shows non-string values as JSON", () => {
    expect(recordToRows({ n: 5, t: true }).map((r) => r.value)).toEqual(["5", "true"]);
    expect(recordToRows(undefined)).toEqual([]);
  });
  it("finds repeated names", () => {
    expect(duplicateKeys([emptyRow("A"), emptyRow("B"), emptyRow("A")])).toEqual(["A"]);
    expect(duplicateKeys([emptyRow("Auth"), emptyRow("auth")])).toEqual([]);
    expect(duplicateKeys([emptyRow("Auth"), emptyRow("auth")], true)).toEqual(["auth"]);
    expect(duplicateKeys([emptyRow(""), emptyRow("")])).toEqual([]);
  });
});
