import { useMemo } from "react";
import { Button, Icon, IconButton, Led, Meter, Pill } from "../../../shared/ui";
import type { AgentProfile, ApprovalRequest, InferenceProfile, RunState, Session } from "../../../shared/api/types";
import type { ApprovalItem } from "../model/items";
import { WorktreePanel } from "./WorktreePanel";

/** Right-hand panel of the chat: runtime and model, tok/s, context, plan, worktree and pending confirmations. */
export function Inspector({ runState, agent, profile, pending, onRespond, onClose, running, session }: {
  runState: RunState;
  agent: AgentProfile | null;
  profile: InferenceProfile | null;
  pending: ApprovalItem[];
  onRespond: (r: ApprovalRequest, d: "allow" | "deny") => void;
  onClose: () => void;
  running: boolean;
  session: Session | null;
}) {
  const u = runState.usage;
  const ctxPct = u?.context_used && u.context_max ? u.context_used / u.context_max : 0;
  const todos = useMemo(() => runState.todos ?? [], [runState.todos]);
  return (
    <aside className="inspector" aria-label="Pannello del run">
      <div className="row">
        <h3 className="grow">Run</h3>
        {running ? <Pill tone="run"><Led tone="run" small />In esecuzione</Pill> : <span className="faint text-xs">inattivo</span>}
        <IconButton className="only-narrow" aria-label="Chiudi pannello" icon="close" onClick={onClose} />
      </div>
      <dl className="kv">
        <dt>Runtime</dt><dd className="mono">{runState.runtime ?? profile?.runtime_id ?? "—"}</dd>
        <dt>Modello</dt><dd className="mono">{runState.model ?? profile?.model ?? "—"}</dd>
        <dt>Agente</dt><dd>{agent?.name ?? "—"}</dd>
        <dt>Sandbox</dt><dd>{agent?.sandbox ?? "—"}</dd>
      </dl>
      <div className="stat-row">
        <div className="stat"><b>{u?.tokens_per_s !== undefined ? u.tokens_per_s.toFixed(1) : "—"}</b><span>tok/s</span></div>
        <div className="stat"><b>{u ? (u.prompt_tokens / 1000).toFixed(1) + "k" : "—"}</b><span>prompt</span></div>
        <div className="stat"><b>{u ? (u.completion_tokens >= 1000 ? (u.completion_tokens / 1000).toFixed(1) + "k" : u.completion_tokens) : "—"}</b><span>output</span></div>
      </div>
      <div className="stack">
        <div className="row row-between">
          <h3>Contesto</h3>
          <span className="mono faint text-xs">{u?.context_used ?? 0} / {u?.context_max ?? profile?.params.context ?? "—"}</span>
        </div>
        <Meter label="Contesto usato" value={u?.context_used ?? 0} max={u?.context_max ?? 1} tone={ctxPct > 0.85 ? "crit" : ctxPct > 0.7 ? "warn" : undefined} />
        <span className="faint text-xs">Compattazione automatica all'80%</span>
      </div>
      <div className="stack">
        <h3>Piano</h3>
        {todos.length === 0 && <span className="faint text-sm">L'agente pubblica qui i suoi passi.</span>}
        {todos.map((t, i) => (
          <div key={i} className={`todo ${t.status}`}>
            {t.status === "completed" ? <Icon name="check" size={15} /> : <Led tone={t.status === "in_progress" ? "run" : "off"} className="todo-led" />}
            <span>{t.content}</span>
          </div>
        ))}
      </div>
      {session?.worktree && <WorktreePanel key={session.id} session={session} running={running} />}
      {pending.length > 0 && (
        <div className="stack">
          <h3>Conferme in attesa</h3>
          {pending.map((p) => (
            <div key={p.req.id} className="approval approval-compact">
              <span className="mono text-xs">{p.req.tool}</span>
              <div className="row">
                <Button variant="primary" size="sm" onClick={() => onRespond(p.req, "allow")}>Consenti</Button>
                <Button variant="danger" size="sm" onClick={() => onRespond(p.req, "deny")}>Nega</Button>
              </div>
            </div>
          ))}
        </div>
      )}
    </aside>
  );
}
