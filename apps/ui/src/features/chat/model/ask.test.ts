import { describe, expect, it } from "vitest";
import type { AgEvent, ApprovalRequest, ChatMessage, Question, SecretRequest } from "../../../shared/api/types";
import { historyToItems } from "./history";
import { readsExternalContent, type Item, type QuestionItem, type SecretItem, type ToolItem } from "./items";
import { reduce, settle } from "./reduce";

const fold = (events: AgEvent[], from: Item[] = []): Item[] => events.reduce<Item[]>((items, e) => reduce(items, e), from);

const question: Question = {
  id: "ask_1", run_id: "run_1", kind: "choice", title: "Quale modello?", message: "Ho trovato due varianti adatte a questo computer.",
  options: [{ id: "q4", label: "Q4_K_M · 4,7 GB", description: "Consigliata" }, { id: "q8", label: "Q8_0 · 8,5 GB", description: null }],
  multiple: false, allow_other: false, fields: [], confirm_label: null, deny_label: null, expires_at: "2026-11-14T10:05:00Z",
};
const questionArgs = { kind: "choice", title: "Quale modello?", message: question.message, options: [{ id: "q4", label: "Q4_K_M · 4,7 GB" }, { id: "q8", label: "Q8_0 · 8,5 GB" }] };

const secret: SecretRequest = {
  id: "sec_1", run_id: "run_1", target: "provider_token:hf", label: "Token di Hugging Face", reason: "Serve per scaricare modelli riservati",
  destination: "Hugging Face (huggingface.co)", expires_at: "2026-11-14T10:05:00Z",
};

const askCall = (id = "c1"): AgEvent[] => [
  { type: "TOOL_CALL_START", toolCallId: id, toolCallName: "ask_user" },
  { type: "TOOL_CALL_ARGS", toolCallId: id, delta: JSON.stringify(questionArgs) },
  { type: "TOOL_CALL_END", toolCallId: id },
];
const secretCall = (id = "c2"): AgEvent[] => [
  { type: "TOOL_CALL_START", toolCallId: id, toolCallName: "aiw_secret_request" },
  { type: "TOOL_CALL_ARGS", toolCallId: id, delta: JSON.stringify({ target: secret.target, label: secret.label, reason: secret.reason }) },
  { type: "TOOL_CALL_END", toolCallId: id },
];

describe("question rows (live)", () => {
  it("replaces the card of the ask_user call with one row, so a question is shown once", () => {
    const items = fold([...askCall(), { type: "CUSTOM", name: "aiw.question", value: question }]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "question", id: "ask_1", callId: "c1", q: { kind: "choice", message: question.message } });
    expect((items[0] as QuestionItem).outcome).toBeUndefined();
  });

  it("closes the row with the answer when the event says so, and the later tool result changes nothing", () => {
    const answered = { id: "ask_1", outcome: "answered", answer: { selected: ["q4"], other: null, confirmed: null, values: null } };
    const items = fold([
      ...askCall(), { type: "CUSTOM", name: "aiw.question", value: question }, { type: "CUSTOM", name: "aiw.question_resolved", value: answered },
      { type: "TOOL_CALL_RESULT", messageId: "m", toolCallId: "c1", content: '{"outcome":"answered","selected":["q4"],"other":null,"confirmed":null,"values":null}' },
    ]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "question", outcome: "answered", answer: { selected: ["q4"], other: null } });
  });

  it("keeps the row when the same question arrives again (events replayed on re-attach)", () => {
    const once = fold([...askCall(), { type: "CUSTOM", name: "aiw.question", value: question }]);
    expect(reduce(once, { type: "CUSTOM", name: "aiw.question", value: question })).toBe(once);
  });

  it("closes the row from the tool result when the closing event was missed", () => {
    const items = fold([
      ...askCall(), { type: "CUSTOM", name: "aiw.question", value: question },
      { type: "TOOL_CALL_RESULT", messageId: "m", toolCallId: "c1", content: '{"outcome":"declined"}' },
    ]);
    expect(items[0]).toMatchObject({ kind: "question", outcome: "declined", answer: null });
  });

  it("an unanswered question is closed as cancelled when the run ends, whatever the way it ends", () => {
    const open = fold([...askCall(), { type: "CUSTOM", name: "aiw.question", value: question }]);
    for (const end of [{ type: "RUN_FINISHED", threadId: "t", runId: "r" }, { type: "RUN_ERROR", message: "annullato", code: "cancelled" }] as AgEvent[]) {
      expect(reduce(open, end)[0]).toMatchObject({ kind: "question", outcome: "cancelled" });
    }
    expect(settle([{ kind: "question", id: "x", q: question, outcome: "answered", answer: null }])).toHaveLength(1);
  });

  it("without a tool call to stand for, the row is added at the end", () => {
    const items = fold([{ type: "TEXT_MESSAGE_START", messageId: "m", role: "assistant" }, { type: "CUSTOM", name: "aiw.question", value: question }]);
    expect(items.map((i) => i.kind)).toEqual(["text", "question"]);
    expect((items[1] as QuestionItem).callId).toBeUndefined();
  });

  it("ignores a question that cannot be shown", () => {
    expect(fold([{ type: "CUSTOM", name: "aiw.question", value: { id: "a", kind: "poll", message: "x" } }])).toEqual([]);
    expect(fold([{ type: "CUSTOM", name: "aiw.question", value: null }])).toEqual([]);
  });

  it("an ask_user call that failed or was refused keeps its tool card (no question event, no row)", () => {
    const items = fold([...askCall(), { type: "TOOL_CALL_RESULT", messageId: "m", toolCallId: "c1", content: '{"outcome":"unsupported","hint":"Fai la domanda in testo e aspetta il prossimo messaggio."}' }]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "tool", name: "ask_user", status: "done" });
  });

  it("two questions in one turn each take the place of their own call", () => {
    const second: Question = { ...question, id: "ask_2", message: "E la quantizzazione?" };
    const items = fold([
      ...askCall("c1"), { type: "CUSTOM", name: "aiw.question", value: question }, { type: "CUSTOM", name: "aiw.question_resolved", value: { id: "ask_1", outcome: "declined", answer: null } },
      { type: "TOOL_CALL_RESULT", messageId: "m", toolCallId: "c1", content: '{"outcome":"declined"}' },
      ...askCall("c3"), { type: "CUSTOM", name: "aiw.question", value: second },
    ]);
    expect(items.map((i) => (i.kind === "question" ? `${i.id}:${i.callId}:${i.outcome ?? "open"}` : i.kind))).toEqual(["ask_1:c1:declined", "ask_2:c3:open"]);
  });
});

describe("secret rows (live)", () => {
  it("replaces the call card with one row; the row and its events never hold a value", () => {
    const items = fold([...secretCall(), { type: "CUSTOM", name: "aiw.secret_request", value: secret }]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "secret", id: "sec_1", callId: "c2", req: { target: "provider_token:hf", destination: "Hugging Face (huggingface.co)" } });
    expect(Object.keys((items[0] as SecretItem).req).sort()).toEqual(["destination", "expires_at", "id", "label", "reason", "run_id", "target"]);
  });

  it("is closed by the event or, failing that, by the result of the call", () => {
    const stored = fold([...secretCall(), { type: "CUSTOM", name: "aiw.secret_request", value: secret }, { type: "CUSTOM", name: "aiw.secret_resolved", value: { id: "sec_1", outcome: "stored" } }]);
    expect(stored[0]).toMatchObject({ kind: "secret", outcome: "stored" });
    const byResult = fold([...secretCall(), { type: "CUSTOM", name: "aiw.secret_request", value: secret }, { type: "TOOL_CALL_RESULT", messageId: "m", toolCallId: "c2", content: '{"outcome":"declined"}' }]);
    expect(byResult[0]).toMatchObject({ kind: "secret", outcome: "declined" });
  });

  it("is cancelled when the run ends before the secret is provided", () => {
    const open = fold([...secretCall(), { type: "CUSTOM", name: "aiw.secret_request", value: secret }]);
    expect(reduce(open, { type: "RUN_FINISHED", threadId: "t", runId: "r" })[0]).toMatchObject({ kind: "secret", outcome: "cancelled" });
  });
});

describe("conversations loaded from the stored messages", () => {
  const stamp = "2026-11-14T10:00:00Z";
  const storedAsk = (result?: string): ChatMessage[] => [
    { id: "m1", role: "user", content: "Scarica un modello", created_at: stamp },
    { id: "m2", role: "assistant", content: "", tool_calls: [{ id: "c1", name: "ask_user", arguments: JSON.stringify(questionArgs) }], created_at: stamp },
    ...(result === undefined ? [] : [{ id: "m3", role: "tool" as const, content: result, tool_call_id: "c1", created_at: stamp }]),
  ];

  it("shows an answered question as a closed row with the choice made", () => {
    const items = historyToItems(storedAsk('{"outcome":"answered","selected":["q4"],"other":null,"confirmed":null,"values":null}'));
    const row = items.find((i) => i.kind === "question") as QuestionItem;
    expect(row).toMatchObject({ outcome: "answered", answer: { selected: ["q4"] }, q: { message: question.message, title: "Quale modello?" } });
    expect(row.q.options.map((o) => o.label)).toEqual(["Q4_K_M · 4,7 GB", "Q8_0 · 8,5 GB"]);
    expect(items.some((i) => i.kind === "tool")).toBe(false);
  });

  it("shows the other outcomes the same way, and a question left open by a stopped run as without an answer", () => {
    expect(historyToItems(storedAsk('{"outcome":"declined"}')).find((i) => i.kind === "question")).toMatchObject({ outcome: "declined", answer: null });
    expect(historyToItems(storedAsk('{"outcome":"timeout"}')).find((i) => i.kind === "question")).toMatchObject({ outcome: "timeout" });
    expect(historyToItems(storedAsk(undefined)).find((i) => i.kind === "question")).toMatchObject({ outcome: "cancelled" });
  });

  it("keeps the tool card when the call failed, was not shown, or its arguments do not parse", () => {
    for (const result of ['{"outcome":"unsupported","hint":"x"}', "ERRORE: domanda non valida"]) {
      const items = historyToItems(storedAsk(result));
      expect(items.find((i) => i.kind === "tool")).toMatchObject({ name: "ask_user" });
      expect(items.some((i) => i.kind === "question")).toBe(false);
    }
    const broken = storedAsk('{"outcome":"declined"}');
    broken[1].tool_calls![0].arguments = "{non json";
    expect(historyToItems(broken).find((i) => i.kind === "tool")).toMatchObject({ name: "ask_user" });
  });

  it("shows a stored request for a secret as a closed row: label, reason and how it ended, nothing else", () => {
    const msgs: ChatMessage[] = [
      { id: "m1", role: "user", content: "Aggiungi il token di Hugging Face", created_at: stamp },
      { id: "m2", role: "assistant", content: "", tool_calls: [{ id: "c2", name: "aiw_secret_request", arguments: JSON.stringify({ target: "provider_token:hf", label: "Token di Hugging Face", reason: "Serve per scaricare modelli riservati" }) }], created_at: stamp },
      { id: "m3", role: "tool", content: '{"outcome":"stored","target":"provider_token:hf"}', tool_call_id: "c2", created_at: stamp },
    ];
    const row = historyToItems(msgs).find((i) => i.kind === "secret") as SecretItem;
    expect(row).toMatchObject({ outcome: "stored", req: { label: "Token di Hugging Face", target: "provider_token:hf", destination: "" } });
    const declined = historyToItems([msgs[0], msgs[1], { ...msgs[2], content: '{"outcome":"declined"}' }]).find((i) => i.kind === "secret");
    expect(declined).toMatchObject({ outcome: "declined" });
  });

  it("a live conversation and the same one reloaded show the same rows", () => {
    const live = fold([
      { type: "TEXT_MESSAGE_START", messageId: "m", role: "assistant" }, { type: "TEXT_MESSAGE_END", messageId: "m" },
      ...askCall(), { type: "CUSTOM", name: "aiw.question", value: question },
      { type: "CUSTOM", name: "aiw.question_resolved", value: { id: "ask_1", outcome: "answered", answer: { selected: ["q8"], other: null, confirmed: null, values: null } } },
      { type: "TOOL_CALL_RESULT", messageId: "m", toolCallId: "c1", content: '{"outcome":"answered","selected":["q8"],"other":null,"confirmed":null,"values":null}' },
    ]);
    const reloaded = historyToItems(storedAsk('{"outcome":"answered","selected":["q8"],"other":null,"confirmed":null,"values":null}'));
    const shape = (i: Item) => (i.kind === "question" ? { kind: i.kind, outcome: i.outcome, selected: i.answer?.selected, message: i.q.message } : null);
    expect(live.filter((i) => i.kind === "question").map(shape)).toEqual(reloaded.filter((i) => i.kind === "question").map(shape));
  });
});

describe("approvals after reading from outside", () => {
  const req = (tool: string): ApprovalRequest => ({ id: "a1", run_id: "r", tool, args: {}, reason: "x", risk: "medium" });
  const tool = (name: string): AgEvent[] => [
    { type: "TOOL_CALL_START", toolCallId: `t-${name}`, toolCallName: name },
    { type: "TOOL_CALL_RESULT", messageId: "m", toolCallId: `t-${name}`, content: "ok" },
  ];
  const approval = (events: AgEvent[], from: Item[] = [{ kind: "user", id: "u", text: "vai" }]) => fold([...events, { type: "CUSTOM", name: "aiw.approval_request", value: req("aiw_model_download") }], from).find((i) => i.kind === "approval");

  it("marks the request when a web, browser, mail or MCP tool ran earlier in the turn", () => {
    for (const name of ["web_search", "web_fetch", "browser_navigate", "mail_read", "mcp__github__list_issues"]) {
      expect(approval(tool(name)), name).toMatchObject({ afterExternal: true });
    }
  });
  it("marks it after a sub-agent too", () => {
    expect(approval([{ type: "SUBAGENT_STARTED", subagentId: "s", name: "Ricerca" }])).toMatchObject({ afterExternal: true });
  });
  it("does not mark it for local tools, or when the external read was in an earlier turn", () => {
    expect(approval(tool("read_file"))).not.toHaveProperty("afterExternal");
    expect(approval([])).not.toHaveProperty("afterExternal");
    const before: Item[] = [{ kind: "tool", id: "old", name: "web_fetch", args: "", status: "done" }, { kind: "user", id: "u2", text: "e poi?" }];
    expect(approval([], before)).not.toHaveProperty("afterExternal");
  });
  it("readsExternalContent looks only at the current turn", () => {
    const web: ToolItem = { kind: "tool", id: "w", name: "web_search", args: "", status: "done" };
    expect(readsExternalContent([{ kind: "user", id: "u", text: "x" }, web])).toBe(true);
    expect(readsExternalContent([web, { kind: "user", id: "u", text: "x" }])).toBe(false);
    expect(readsExternalContent([web, { kind: "action", id: "a", name: "ok", context: {} }])).toBe(false);
    expect(readsExternalContent([])).toBe(false);
  });
});
