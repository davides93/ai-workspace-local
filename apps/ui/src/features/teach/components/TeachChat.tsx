import { useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { qk } from "../../../shared/query/keys";
import { Composer } from "../../chat/components/Composer";
import { ConversationLog, type ConversationLogHandle } from "../../chat/components/ConversationLog";
import { useDraft } from "../../chat/useLayout";
import { useConversation } from "../../chat/useConversation";
import { useRefreshMemories } from "../api";
import { EXAMPLES } from "../model/memories";

import { t as tr } from "../../../shared/i18n";
/** The name of the CUSTOM event the engine sends after a memory tool ran (crates/aiw-agent/src/runner.rs `MEMORY_EVENT`). */
export const MEMORY_EVENT = "aiw.memory";

/** The conversation with the teacher agent: the usual chat components on one persistent session. */
export function TeachChat({ sessionId }: { sessionId: string }) {
  const qc = useQueryClient();
  const refresh = useRefreshMemories();
  const conv = useConversation(sessionId, {
    // The session was deleted from another client: the page asks for the list again and makes a new one.
    onGone: () => void qc.invalidateQueries({ queryKey: qk.sessions.all() }),
    // "Cosa so" follows what the assistant saves, forgets or corrects while it works.
    onEvent: (e) => {
      if (e.type === "RUN_FINISHED" || (e.type === "CUSTOM" && e.name === MEMORY_EVENT)) refresh();
    },
  });
  const [input, setInput] = useDraft(sessionId);
  const [focusTick, setFocusTick] = useState(0);
  const log = useRef<ConversationLogHandle>(null);

  const send = (text: string, restore?: () => void) => {
    const t = text.trim();
    if (!t || conv.running) return;
    log.current?.scrollToBottom();
    conv.send(t, restore);
  };
  const ready = conv.history.status === "ready";

  return (
    <>
      <ConversationLog
        ref={log} items={conv.items} state={conv.history} onRetry={conv.reload} busy={conv.running} respondingId={conv.respondingId}
        onRespond={conv.respond} onAction={conv.sendAction} notice={null} sessionKey={sessionId}
        empty={
          <div className="ws-hero">
            <h2>{tr("teach.cosa_devo_ricordare")}</h2>
            <p className="muted">{tr("teach.scrivimi_cose_su_di_te")}</p>
            <ul className="ws-suggestions" aria-label={tr("teach.frasi_di_esempio")}>
              {EXAMPLES.map((s) => (
                <li key={s}><button type="button" className="btn" disabled={!ready || conv.running} onClick={() => send(s)}>{s}</button></li>
              ))}
            </ul>
          </div>
        }
      />
      <Composer
        value={input} onChange={setInput} onStop={conv.stop} running={conv.running} disabled={!ready} focusKey={`${sessionId}:${focusTick}`}
        onSend={() => {
          const text = input;
          setInput("");
          setFocusTick((n) => n + 1);
          send(text, () => setInput(text));
        }}
        placeholder={ready ? tr("teach.scrivi_cosa_devo_ricordare") : tr("teach.carico_la_conversazione")}
      />
    </>
  );
}
