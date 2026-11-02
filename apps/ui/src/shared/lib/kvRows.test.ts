import { describe, expect, it } from "vitest";
import { duplicateKeys, emptyRow, isKeptRow, recordToRows, renameRow, rowsToRecord, strayMasks } from "./kvRows";

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
  it("marks the entries the server sent as the mask, so they can be told from a typed value", () => {
    const rows = recordToRows({ Authorization: "••••••", Accept: "json", Odd: 5 }, "••••••");
    expect(rows.map((r) => r.kept)).toEqual(["Authorization", undefined, undefined]);
    expect(rows.map((r) => isKeptRow(r, "••••••"))).toEqual([true, false, false]);
    expect(recordToRows({ A: "••••••" })[0].kept).toBeUndefined();
  });
  it("loses the kept secret when the row is renamed or the value changes, and flags a mask with nothing behind it", () => {
    const [row] = recordToRows({ Authorization: "••••••" }, "••••••");
    expect(strayMasks([row], "••••••")).toEqual([]);
    expect(isKeptRow({ ...row, key: " Authorization " }, "••••••")).toBe(true);
    expect(strayMasks([{ ...row, key: "X-Token" }], "••••••")).toEqual(["X-Token"]);
    expect(strayMasks([emptyRow("A", "••••••"), emptyRow("", "••••••")], "••••••")).toEqual(["A"]);
    expect(isKeptRow({ ...row, value: "new" }, "••••••")).toBe(false);
  });
  it("empties a kept secret when its row is renamed, and offers it back under the old name", () => {
    const [row] = recordToRows({ Authorization: "••••••" }, "••••••");
    const renamed = renameRow(row, "X-Token", "••••••");
    expect(renamed).toMatchObject({ key: "X-Token", value: "", kept: "Authorization" });
    expect(strayMasks([renamed], "••••••")).toEqual([]);
    // Back to the old name: the row can keep the stored secret again (the editor offers "Mantieni").
    expect(renameRow(renamed, "Authorization", "••••••").kept).toBe("Authorization");
    // Same name (even with spaces around it), no mask, or a row that never held a secret: nothing is emptied.
    expect(renameRow(row, " Authorization ", "••••••").value).toBe("••••••");
    expect(renameRow(row, "X-Token").value).toBe("••••••");
    expect(renameRow(emptyRow("A", "1"), "B", "••••••").value).toBe("1");
  });
});
