import { describe, expect, it } from "vitest";
import type { Item } from "./items";
import { approvalAnnouncement, replyToSpeak } from "./voiceReply";

const user = (text: string): Item => ({ kind: "user", id: `u-${text}`, text });
const text = (t: string, id = t): Item => ({ kind: "text", id, text: t, streaming: false });

describe("replyToSpeak", () => {
  it("is the text after the last user message, in one piece", () => {
    const items: Item[] = [user("ciao"), text("Vecchia risposta"), user("e ora?"), text("Prima parte"), { kind: "note", id: "n", text: "x" }, text("Seconda parte")];
    expect(replyToSpeak(items)).toBe("Prima parte\n\nSeconda parte");
  });

  it("ignores tools and reasoning, which are not read aloud", () => {
    const items: Item[] = [
      user("fai"), { kind: "reasoning", id: "r", text: "ragiono", streaming: false },
      { kind: "tool", id: "t", name: "shell", args: "{}", status: "done" }, text("Fatto"),
    ];
    expect(replyToSpeak(items)).toBe("Fatto");
  });

  it("says why the run did not finish when there is no text", () => {
    expect(replyToSpeak([user("fai"), { kind: "error", id: "e", message: "modello non raggiungibile" }])).toBe("Non sono riuscito a completare: modello non raggiungibile");
    expect(replyToSpeak([user("fai")])).toBe("");
    expect(replyToSpeak([])).toBe("");
  });

  it("prefers the reply over an error that follows it", () => {
    expect(replyToSpeak([user("a"), text("Risposta"), { kind: "error", id: "e", message: "x" }])).toBe("Risposta");
  });

  it("reads the whole history when nobody has written yet", () => {
    expect(replyToSpeak([text("Benvenuto")])).toBe("Benvenuto");
  });
});

describe("approvalAnnouncement", () => {
  it("names the reason", () => {
    expect(approvalAnnouncement({ reason: "eseguire rm -r build" })).toBe("Serve la tua conferma: eseguire rm -r build");
  });
});
