import { useMemo, type ReactNode } from "react";
import { formatNumber, formatPercent } from "../../../shared/lib/format";
import { Icon, KeyValue, Led, Meter, Pill } from "../../../shared/ui";
import { ApprovalCard } from "../../approvals/components/ApprovalCard";
import type { AgentProfile, ApprovalRequest, InferenceProfile, RunState, Session } from "../../../shared/api/types";
import type { ApprovalItem } from "../model/items";
import { modeInfo, sandboxInfo } from "../model/labels";
import { WorktreePanel } from "./WorktreePanel";
import { CheckpointPanel } from "./CheckpointPanel";

const tokens = (n: number) => (n >= 1000 ? `${formatNumber(n / 1000, 1)}k` : String(n));

function Section({ title, aside, children }: { title: string; aside?: ReactNode; children: ReactNode }) {
  return (
    <section className="ws-insp-section">
      <div className="ws-insp-head">
        <h3>{title}</h3>
        {aside}
      </div>
      {children}
    </section>
  );
}

/**
 * Content of the run panel: pending confirmations first, then runtime and model, speed, context,
 * the agent's plan and the worktree. Rendered inline on desktop, in a drawer on tablets and in a
 * sheet on phones (the wrappers live in `ChatView`).
 */
export function InspectorBody({ runState, agent, profile, session, pending, running, respondingId, compactAt, onRespond }: {
  runState: RunState;
  agent: AgentProfile | null;
  profile: InferenceProfile | null;
  session: Session | null;
  pending: ApprovalItem[];
  running: boolean;
  respondingId?: string | null;
  /** Fraction of the context window at which the backend compacts the history. */
  compactAt: number;
  onRespond: (r: ApprovalRequest, d: "allow" | "deny", remember?: boolean) => void;
}) {
  const u = runState.usage;
  const ctxMax = u?.context_max ?? profile?.params.context;
  const ctxPct = u?.context_used && ctxMax ? u.context_used / ctxMax : 0;
  const todos = useMemo(() => runState.todos ?? [], [runState.todos]);
  const shared = useMemo(() => Object.entries(runState.shared ?? {}), [runState.shared]);
  const doneCount = todos.filter((t) => t.status === "completed").length;
  const mode = modeInfo(agent?.mode);
  const sandbox = sandboxInfo(agent?.sandbox);
  return (
    <div className="ws-insp">
      {pending.length > 0 && (
        <Section title="Conferme in attesa" aside={<Pill tone="warn">{pending.length}</Pill>}>
          {pending.map((p) => (
            <ApprovalCard key={p.req.id} request={p.req} variant="compact" pending={respondingId === p.req.id} onRespond={(d, remember) => onRespond(p.req, d, remember)} />
          ))}
        </Section>
      )}

      <Section title="Run" aside={running ? <Pill tone="run"><Led tone="run" small />In esecuzione</Pill> : <span className="faint text-xs">inattivo</span>}>
        <KeyValue
          items={[
            ["Runtime", <span className="mono">{runState.runtime ?? profile?.runtime_id ?? "—"}</span>],
            ["Modello", <span className="mono">{runState.model ?? profile?.model ?? "—"}</span>],
            ["Agente", agent?.name ?? "—"],
            ["Approvazione", <span title={mode.hint}>{mode.label}</span>],
            ["Sandbox", <span title={sandbox.hint}>{sandbox.label}</span>],
          ]}
        />
      </Section>

      <Section title="Prestazioni">
        <div className="ws-stats">
          <div className="ws-stat"><b>{u?.tokens_per_s !== undefined ? formatNumber(u.tokens_per_s, 1) : "—"}</b><span>tok/s</span></div>
          <div className="ws-stat"><b>{u ? tokens(u.prompt_tokens) : "—"}</b><span>prompt</span></div>
          <div className="ws-stat"><b>{u ? tokens(u.completion_tokens) : "—"}</b><span>output</span></div>
        </div>
      </Section>

      <Section title="Contesto" aside={<span className="mono faint text-xs num">{u?.context_used ?? 0} / {ctxMax ?? "—"}</span>}>
        <Meter label="Contesto usato" value={u?.context_used ?? 0} max={ctxMax ?? 1} tone={ctxPct > 0.85 ? "crit" : ctxPct > 0.7 ? "warn" : undefined} />
        <span className="faint text-xs">Compattazione automatica al {formatPercent(compactAt)}</span>
      </Section>

      <Section title="Piano" aside={todos.length > 0 ? <span className="faint text-xs num">{doneCount} di {todos.length}</span> : undefined}>
        {todos.length === 0 ? (
          <span className="faint text-sm">L'agente pubblica qui i suoi passi.</span>
        ) : (
          <ul className="ws-todos">
            {todos.map((t, i) => (
              <li key={i} className={`ws-todo ${t.status}`}>
                {t.status === "completed" ? <Icon name="check" size={15} /> : <Led tone={t.status === "in_progress" ? "run" : "off"} className="ws-todo-led" />}
                <span>{t.content}</span>
                <span className="sr-only">{t.status === "completed" ? " (completato)" : t.status === "in_progress" ? " (in corso)" : " (da fare)"}</span>
              </li>
            ))}
          </ul>
        )}
      </Section>

      {shared.length > 0 && (
        <Section title="Stato condiviso" aside={<span className="faint text-xs num">{shared.length} {shared.length === 1 ? "chiave" : "chiavi"}</span>}>
          <KeyValue items={shared.map(([k, v]): [string, ReactNode] => [k, <span className="mono text-xs" title={JSON.stringify(v, null, 2)}>{typeof v === "string" ? v : JSON.stringify(v)}</span>])} />
        </Section>
      )}

      {session?.worktree && <WorktreePanel key={session.id} session={session} running={running} />}
      {session && <CheckpointPanel key={`ck-${session.id}`} sessionId={session.id} running={running} />}
    </div>
  );
}
