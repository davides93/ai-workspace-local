import { describe, expect, it } from "vitest";
import { reasoningLabel, type Item } from "./items";
import { reduce } from "./reduce";

type Reasoning = Extract<Item, { kind: "reasoning" }>;
const row = (over: Partial<Reasoning> = {}): Reasoning => ({ kind: "reasoning", id: "r1", text: "hmm", streaming: false, ...over });

describe("reasoning row", () => {
  it("says it is thinking while it streams", () => {
    expect(reasoningLabel(row({ streaming: true, startedAt: 1000 }))).toBe("Sta ragionando…");
  });
  it("says how long it took once finished, at least one second", () => {
    expect(reasoningLabel(row({ startedAt: 1000, endedAt: 8400 }))).toBe("Ha ragionato per 7 secondi");
    expect(reasoningLabel(row({ startedAt: 1000, endedAt: 1200 }))).toBe("Ha ragionato per 1 secondo");
  });
  it("keeps the plain name when the duration is not known (stored history)", () => {
    expect(reasoningLabel(row())).toBe("Ragionamento");
  });
  it("takes its times from the events of the run", () => {
    const items = [
      { type: "REASONING_START" as const, messageId: "r1", timestamp: 5000 },
      { type: "REASONING_MESSAGE_CONTENT" as const, messageId: "r1", delta: "pensa" },
      { type: "REASONING_END" as const, messageId: "r1", timestamp: 9000 },
    ].reduce<Item[]>(reduce, []);
    expect(items[0]).toMatchObject({ startedAt: 5000, endedAt: 9000, streaming: false });
    expect(reasoningLabel(items[0] as Reasoning)).toBe("Ha ragionato per 4 secondi");
  });
});
