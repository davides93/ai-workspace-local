import type { AgEvent, AiwImage, ApprovalRequest } from "../../../shared/api/types";
import { applyA2ui, type A2uiMessage } from "../a2ui";
import { SELF_CHECK, selfCheckText, type Item } from "./items";

/** Folds one AG-UI event into the conversation rows (pure: returns a new array). */
export function reduce(items: Item[], e: AgEvent): Item[] {
  const next = [...items];
  const find = <K extends Item["kind"]>(kind: K, id: string) =>
    next.findIndex((i) => i.kind === kind && "id" in i && i.id === id);
  switch (e.type) {
    case "TEXT_MESSAGE_START":
      next.push({ kind: "text", id: e.messageId, text: "", streaming: true });
      break;
    case "TEXT_MESSAGE_CONTENT": {
      const i = find("text", e.messageId);
      if (i >= 0) next[i] = { ...(next[i] as Extract<Item, { kind: "text" }>), text: (next[i] as Extract<Item, { kind: "text" }>).text + e.delta };
      else next.push({ kind: "text", id: e.messageId, text: e.delta, streaming: true });
      break;
    }
    case "TEXT_MESSAGE_END": {
      const i = find("text", e.messageId);
      if (i >= 0) next[i] = { ...(next[i] as Extract<Item, { kind: "text" }>), streaming: false };
      break;
    }
    case "REASONING_START":
      next.push({ kind: "reasoning", id: e.messageId, text: "", streaming: true });
      break;
    case "REASONING_MESSAGE_CONTENT": {
      const i = find("reasoning", e.messageId);
      if (i >= 0) next[i] = { ...(next[i] as Extract<Item, { kind: "reasoning" }>), text: (next[i] as Extract<Item, { kind: "reasoning" }>).text + e.delta };
      break;
    }
    case "REASONING_END": {
      const i = find("reasoning", e.messageId);
      if (i >= 0) next[i] = { ...(next[i] as Extract<Item, { kind: "reasoning" }>), streaming: false };
      break;
    }
    case "TOOL_CALL_START":
      next.push({ kind: "tool", id: e.toolCallId, name: e.toolCallName, args: "", status: "running" });
      break;
    case "TOOL_CALL_ARGS": {
      const i = find("tool", e.toolCallId);
      if (i >= 0) next[i] = { ...(next[i] as Extract<Item, { kind: "tool" }>), args: (next[i] as Extract<Item, { kind: "tool" }>).args + e.delta };
      break;
    }
    case "TOOL_CALL_RESULT": {
      const i = find("tool", e.toolCallId);
      const denied = /^Negato|^Denied/i.test(e.content);
      if (i >= 0) next[i] = { ...(next[i] as Extract<Item, { kind: "tool" }>), result: e.content, status: denied ? "denied" : "done" };
      break;
    }
    case "SUBAGENT_STARTED":
      next.push({ kind: "subagent", id: e.subagentId, name: e.name });
      break;
    case "SUBAGENT_FINISHED": {
      const i = find("subagent", e.subagentId);
      if (i >= 0) next[i] = { ...(next[i] as Extract<Item, { kind: "subagent" }>), result: e.result ?? "completato" };
      break;
    }
    case "CUSTOM":
      if (e.name === "a2ui") {
        const v = e.value as { surfaceId: string; messages: A2uiMessage[] };
        const i = next.findIndex((x) => x.kind === "surface" && x.id === v.surfaceId);
        if (i >= 0) {
          const it = next[i] as Extract<Item, { kind: "surface" }>;
          const surface = applyA2ui(it.surface, v.messages);
          if (surface) next[i] = { ...it, surface };
        } else {
          const surface = applyA2ui(null, v.messages);
          if (surface) next.push({ kind: "surface", id: v.surfaceId, surface });
        }
      }
      if (e.name === SELF_CHECK) next.push({ kind: "note", id: `sc-${next.length}`, text: selfCheckText((e.value as { failed: string[] }).failed ?? []) });
      if (e.name === "aiw.approval_request") next.push({ kind: "approval", req: e.value as ApprovalRequest });
      if (e.name === "aiw.approval_resolved") {
        const v = e.value as { id: string; allowed: boolean };
        const i = next.findIndex((x) => x.kind === "approval" && x.req.id === v.id);
        if (i >= 0) next[i] = { ...(next[i] as Extract<Item, { kind: "approval" }>), resolved: v.allowed };
      }
      if (e.name === "aiw.image") {
        // Screenshot of a browser or computer-use tool: shown inside the card of that tool call.
        const v = e.value as AiwImage;
        const i = find("tool", v.toolCallId);
        if (i >= 0) {
          const it = next[i] as Extract<Item, { kind: "tool" }>;
          next[i] = { ...it, images: [...(it.images ?? []), { mime: v.mime, data: v.data }] };
        }
      }
      break;
    case "RUN_ERROR":
      next.push({ kind: "error", message: e.message });
      break;
  }
  return next;
}
