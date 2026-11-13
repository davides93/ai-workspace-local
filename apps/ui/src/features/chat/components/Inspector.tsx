import { useMemo, type ReactNode } from "react";
import { formatNumber, formatPercent } from "../../../shared/lib/format";
import { Icon, KeyValue, Led, Meter, Pill } from "../../../shared/ui";
import { ApprovalCard } from "../../approvals/components/ApprovalCard";
import type { AgentProfile, ApprovalRequest, InferenceProfile, RunState, Session } from "../../../shared/api/types";
import type { ApprovalItem } from "../model/items";
import { modeInfo, sandboxInfo } from "../model/labels";
import { WorktreePanel } from "./WorktreePanel";
import { CheckpointPanel } from "./CheckpointPanel";

import { t as tr, t } from "../../../shared/i18n";
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
        <Section title={tr("chat.conferme_in_attesa")} aside={<Pill tone="warn">{pending.length}</Pill>}>
          {pending.map((p) => (
            <ApprovalCard key={p.req.id} request={p.req} variant="compact" pending={respondingId === p.req.id} onRespond={(d, remember) => onRespond(p.req, d, remember)} />
          ))}
        </Section>
      )}

      <Section title={tr("chat.run")} aside={running ? <Pill tone="run"><Led tone="run" small />{t("chat.in_esecuzione")}</Pill> : <span className="faint text-xs">{tr("chat.inattivo")}</span>}>
        <KeyValue
          items={[
            [t("chat.runtime_2"), <span className="mono">{runState.runtime ?? profile?.runtime_id ?? "—"}</span>],
            [t("chat.modello"), <span className="mono">{runState.model ?? profile?.model ?? "—"}</span>],
            [t("chat.agente"), agent?.name ?? "—"],
            [t("chat.approvazione"), <span title={mode.hint}>{mode.label}</span>],
            [t("chat.sandbox"), <span title={sandbox.hint}>{sandbox.label}</span>],
          ]}
        />
      </Section>

      <Section title={tr("chat.prestazioni")}>
        <div className="ws-stats">
          <div className="ws-stat"><b>{u?.tokens_per_s !== undefined ? formatNumber(u.tokens_per_s, 1) : "—"}</b><span>tok/s</span></div>
          <div className="ws-stat"><b>{u ? tokens(u.prompt_tokens) : "—"}</b><span>{tr("chat.prompt")}</span></div>
          <div className="ws-stat"><b>{u ? tokens(u.completion_tokens) : "—"}</b><span>{tr("chat.output")}</span></div>
        </div>
      </Section>

      <Section title={tr("chat.contesto")} aside={<span className="mono faint text-xs num">{u?.context_used ?? 0} / {ctxMax ?? "—"}</span>}>
        <Meter label={tr("chat.contesto_usato")} value={u?.context_used ?? 0} max={ctxMax ?? 1} tone={ctxPct > 0.85 ? "crit" : ctxPct > 0.7 ? "warn" : undefined} />
        <span className="faint text-xs">{tr("chat.compattazione_automatica_al", { p: formatPercent(compactAt) })}</span>
      </Section>

      <Section title={tr("chat.piano")} aside={todos.length > 0 ? <span className="faint text-xs num">{tr("chat.di", { doneCount, length: todos.length })}</span> : undefined}>
        {todos.length === 0 ? (
          <span className="faint text-sm">{tr("chat.l_agente_pubblica_qui_i")}</span>
        ) : (
          <ul className="ws-todos">
            {todos.map((t, i) => (
              <li key={i} className={`ws-todo ${t.status}`}>
                {t.status === "completed" ? <Icon name="check" size={15} /> : <Led tone={t.status === "in_progress" ? "run" : "off"} className="ws-todo-led" />}
                <span>{t.content}</span>
                <span className="sr-only">{t.status === "completed" ? tr("chat.completato") : t.status === "in_progress" ? tr("chat.in_corso") : tr("chat.da_fare")}</span>
              </li>
            ))}
          </ul>
        )}
      </Section>

      {shared.length > 0 && (
        <Section title={tr("chat.stato_condiviso")} aside={<span className="faint text-xs num">{shared.length} {shared.length === 1 ? tr("chat.chiave") : tr("chat.chiavi")}</span>}>
          <KeyValue items={shared.map(([k, v]): [string, ReactNode] => [k, <span className="mono text-xs" title={JSON.stringify(v, null, 2)}>{typeof v === "string" ? v : JSON.stringify(v)}</span>])} />
        </Section>
      )}

      {session?.worktree && <WorktreePanel key={session.id} session={session} running={running} />}
      {session && <CheckpointPanel key={`ck-${session.id}`} sessionId={session.id} running={running} />}
    </div>
  );
}
