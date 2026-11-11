import { ErrorState, Icon, SkeletonLines } from "../../shared/ui";
import "../chat/chat.css";
import { useTeachSession } from "./api";
import { MemoryPanel } from "./components/MemoryPanel";
import { TeachChat } from "./components/TeachChat";
import "./teach.css";

/**
 * `#/teach` — "Insegna": a chat with the teacher agent on the left (on top on phones) and what the
 * assistant knows on the right (below on phones). The conversation is one persistent session.
 */
export function TeachView() {
  const session = useTeachSession();
  return (
    <div className="th">
      <section className="ws-main th-chat" aria-label="Chat con l'Insegnante">
        <header className="th-head">
          <Icon name="bulb" size={20} />
          <div>
            <h1>Insegna</h1>
            <p className="muted text-sm">Racconta all'assistente cosa deve ricordare: lo scrive e lo trovi qui accanto.</p>
          </div>
        </header>
        {session.id ? (
          <TeachChat key={session.id} sessionId={session.id} />
        ) : session.error ? (
          <div className="th-state"><ErrorState title="Non riesco ad aprire la conversazione" error={session.error} onRetry={session.retry} /></div>
        ) : (
          <div className="th-state"><SkeletonLines lines={4} /></div>
        )}
      </section>
      <aside className="th-panel" aria-label="Cosa so">
        <MemoryPanel />
      </aside>
    </div>
  );
}
