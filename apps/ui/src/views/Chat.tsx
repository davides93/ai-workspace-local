import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useApp } from "../state/app";
import { Icon, Markdown, Meter, relTime, useAsync } from "../components/ui";
import {
  A2UI_ACTION_NAME, A2uiSurface, actionText, applyA2ui, describeAction, renderUiToMessages,
  type A2uiAction, type A2uiMessage, type SurfaceState,
} from "../components/A2ui";
import type { AgEvent, AgentProfile, ApprovalRequest, ChatMessage, InferenceProfile, RunState, Session, WorktreeStatus } from "../api/types";

type Item =
  | { kind: "user"; id: string; text: string }
  | { kind: "text"; id: string; text: string; streaming: boolean }
  | { kind: "reasoning"; id: string; text: string; streaming: boolean }
  | { kind: "tool"; id: string; name: string; args: string; result?: string; status: "running" | "done" | "denied" }
  | { kind: "approval"; req: ApprovalRequest; resolved?: boolean }
  | { kind: "subagent"; id: string; name: string; result?: string }
  | { kind: "note"; id: string; text: string }
  | { kind: "surface"; id: string; surface: SurfaceState }
  | { kind: "action"; id: string; name: string; context: Record<string, unknown> }
  | { kind: "error"; message: string };

/** Stored user messages written by the agent loop's self-check (not typed by the user). */
const SELF_CHECK = "aiw.self_check";
const selfCheckText = (failed: string[]) =>
  `Controllo automatico: ${failed.length === 1 ? "un'azione era fallita" : "alcune azioni erano fallite"} (${failed.join(", ")}). L'agente le riprende prima di concludere.`;

function historyToItems(msgs: ChatMessage[]): Item[] {
  const items: Item[] = [];
  for (const m of msgs) {
    if (m.role === "user" && m.name === A2UI_ACTION_NAME) {
      const a = describeAction(m.content);
      items.push(a ? { kind: "action", id: m.id, ...a } : { kind: "user", id: m.id, text: m.content });
    } else if (m.role === "user" && m.name === SELF_CHECK) {
      const failed = [...m.content.matchAll(/^- ([\w.-]+):/gm)].map((x) => x[1]);
      items.push({ kind: "note", id: m.id, text: selfCheckText(failed) });
    } else if (m.role === "user") items.push({ kind: "user", id: m.id, text: m.content });
    else if (m.role === "assistant") {
      if (m.reasoning) items.push({ kind: "reasoning", id: `${m.id}-r`, text: m.reasoning, streaming: false });
      if (m.content) items.push({ kind: "text", id: m.id, text: m.content, streaming: false });
      for (const tc of m.tool_calls ?? []) items.push({ kind: "tool", id: tc.id, name: tc.name, args: tc.arguments, status: "running" });
    } else if (m.role === "tool") {
      const idx = items.findIndex((i) => i.kind === "tool" && i.id === m.tool_call_id);
      const card = items[idx] as Extract<Item, { kind: "tool" }> | undefined;
      if (card) {
        card.result = m.content;
        card.status = "done";
        // A successful render_ui call is shown as the interface it produced.
        if (card.name === "render_ui" && !m.content.startsWith("ERRORE")) {
          try {
            const msgs = renderUiToMessages(JSON.parse(card.args) as Record<string, unknown>);
            const sid = msgs[0].createSurface!.surfaceId;
            const prev = items.findIndex((i) => i.kind === "surface" && i.id === sid);
            if (prev >= 0) {
              const it = items[prev] as Extract<Item, { kind: "surface" }>;
              it.surface = applyA2ui(it.surface, msgs) ?? it.surface;
              items.splice(idx, 1);
            } else {
              const surface = applyA2ui(null, msgs);
              if (surface) items[idx] = { kind: "surface", id: sid, surface };
            }
          } catch {
            /* malformed arguments: keep the tool card */
          }
        }
      }
    }
  }
  return items;
}

function reduce(items: Item[], e: AgEvent): Item[] {
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
      break;
    case "RUN_ERROR":
      next.push({ kind: "error", message: e.message });
      break;
  }
  return next;
}

export function ChatView() {
  const { api, toast } = useApp();
  const sessions = useAsync(() => api.sessions(), [api]);
  const agents = useAsync(() => api.agents(), [api]);
  const profiles = useAsync(() => api.profiles(), [api]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [runId, setRunId] = useState<string | null>(null);
  const [runState, setRunState] = useState<RunState>({});
  const [input, setInput] = useState("");
  const [showInspector, setShowInspector] = useState(false);
  const [showSessions, setShowSessions] = useState(false);
  const [newAgent, setNewAgent] = useState<string>("operator");
  const [isolated, setIsolated] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const unsub = useRef<(() => void) | null>(null);

  const active = sessions.data?.find((s) => s.id === activeId) ?? null;
  const agent = agents.data?.find((a) => a.id === active?.agent_id) ?? null;
  const profile = profiles.data?.find((p) => p.id === agent?.inference_profile) ?? null;

  useEffect(() => {
    if (!activeId && sessions.data?.length) setActiveId(sessions.data[0].id);
  }, [sessions.data, activeId]);

  useEffect(() => {
    if (!activeId) return;
    unsub.current?.();
    setRunId(null);
    setRunState({});
    let alive = true;
    api
      .messages(activeId)
      .then(async (m) => {
        if (!alive) return;
        // A run may still be going on (started elsewhere, or before switching session): re-attach.
        const running = await api.activeRun(activeId).catch(() => null);
        if (!alive) return;
        if (running) {
          // History already contains the user prompt of the running turn; events rebuild the rest.
          setItems(historyToItems(m));
          setRunId(running);
          unsub.current = api.subscribeRun(running, onEventRef.current);
        } else {
          setItems(historyToItems(m));
        }
      })
      .catch((e) => toast(String(e), "crit"));
    return () => {
      alive = false;
    };
  }, [activeId, api, toast]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [items]);

  useEffect(() => () => unsub.current?.(), []);

  const onEventRef = useRef<(e: AgEvent) => void>(() => {});
  const onEvent = useCallback((e: AgEvent) => {
    if (e.type === "STATE_SNAPSHOT") setRunState(e.snapshot);
    if (e.type === "RUN_FINISHED" || e.type === "RUN_ERROR") {
      setRunId(null);
      sessions.reload();
    }
    setItems((prev) => reduce(prev, e));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  onEventRef.current = onEvent;

  const send = async () => {
    const text = input.trim();
    if (!text || !activeId || runId) return;
    setInput("");
    setItems((prev) => [...prev, { kind: "user", id: `u-${Date.now()}`, text }]);
    try {
      const { run_id } = await api.startRun(activeId, text);
      setRunId(run_id);
      unsub.current?.();
      unsub.current = api.subscribeRun(run_id, onEvent);
    } catch (e) {
      toast(`Impossibile avviare il run: ${String((e as Error).message ?? e)}`, "crit");
    }
  };

  const sendAction = async (a: A2uiAction) => {
    if (!activeId || runId) return;
    setItems((prev) => [...prev, { kind: "action", id: `a-${Date.now()}`, name: a.name, context: a.context }]);
    try {
      const { run_id } = await api.startRun(activeId, actionText(a), "a2ui.action");
      setRunId(run_id);
      unsub.current?.();
      unsub.current = api.subscribeRun(run_id, onEvent);
    } catch (e) {
      toast(`Impossibile inviare l'azione: ${String((e as Error).message ?? e)}`, "crit");
    }
  };

  const newSession = async () => {
    try {
      const s = await api.createSession({ agent_id: newAgent, isolation: isolated ? "worktree" : "none" });
      await sessions.reload();
      setActiveId(s.id);
      setShowSessions(false);
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  };

  const respond = async (req: ApprovalRequest, decision: "allow" | "deny", remember = false) => {
    await api.respondApproval(req.id, decision, remember);
    setItems((prev) => prev.map((i) => (i.kind === "approval" && i.req.id === req.id ? { ...i, resolved: decision === "allow" } : i)));
  };

  const pendingApprovals = items.filter((i) => i.kind === "approval" && i.resolved === undefined) as Extract<Item, { kind: "approval" }>[];

  return (
    <div className={`chat ${showInspector ? "show-inspector" : ""} ${showSessions ? "show-sessions" : ""}`}>
      <aside className="sessions" aria-label="Sessioni">
        <div className="sessions-head">
          <h2 className="grow">Sessioni</h2>
          {showSessions && <button className="icon-btn" onClick={() => setShowSessions(false)} aria-label="Chiudi"><Icon name="close" /></button>}
        </div>
        <div style={{ padding: "0 12px 12px", display: "flex", gap: 6 }}>
          <select className="input" aria-label="Agente per la nuova sessione" value={newAgent} onChange={(e) => setNewAgent(e.target.value)}>
            {agents.data?.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </select>
          <button className="btn primary" onClick={newSession} title="Nuova sessione"><Icon name="plus" size={16} /></button>
        </div>
        <label className="check-row" title="L'agente lavora su un branch aiw/… in una copia separata del repository: rivedi le differenze e applicale quando vuoi.">
          <input type="checkbox" checked={isolated} onChange={(e) => setIsolated(e.target.checked)} />
          <Icon name="branch" size={14} /> Isola in un worktree git
        </label>
        <div className="sessions-list">
          {sessions.data?.map((s) => (
            <SessionItem key={s.id} s={s} agents={agents.data ?? []} active={s.id === activeId} onClick={() => { setActiveId(s.id); setShowSessions(false); }} />
          ))}
          {sessions.data?.length === 0 && <div className="empty">Nessuna sessione. Scegli un agente e premi +.</div>}
        </div>
      </aside>

      <section className="convo" aria-label="Conversazione">
        <div className="convo-head">
          <button className="icon-btn only-narrow" onClick={() => setShowSessions(true)} aria-label="Sessioni"><Icon name="list" /></button>
          <h2 className="grow">{active?.title ?? "Workspace"}</h2>
          {agent && <span className="pill neutral"><Icon name="agent" size={13} /> {agent.name}</span>}
          {agent && <span className="pill info" title="Modalità di approvazione"><Icon name="shield" size={13} /> {agent.mode}</span>}
          {active?.worktree && (
            <button className="pill neutral mono pill-btn" onClick={() => setShowInspector(true)} title={`Worktree isolato: ${active.worktree.path}`}>
              <Icon name="branch" size={13} /> {active.worktree.branch}
            </button>
          )}
          {runId ? <span className="pill run"><span className="led run" style={{ width: 7, height: 7, boxShadow: "none" }} />In esecuzione</span> : null}
          <button className="icon-btn" onClick={() => setShowInspector((v) => !v)} aria-label="Pannello run"><Icon name="panel" /></button>
        </div>
        <div className="convo-scroll" ref={scrollRef}>
          {items.length === 0 && (
            <div className="empty" style={{ alignSelf: "center", maxWidth: 560, marginTop: 40 }}>
              <h2 style={{ color: "var(--fg)" }}>Cosa facciamo, in locale?</h2>
              <p>L'agente lavora con i modelli di questo dispositivo e chiede conferma prima di modificare file o eseguire comandi.</p>
              <div className="row" style={{ marginTop: 6 }}>
                {["Riordina i PDF in Downloads per anno e crea un indice", "Confronta i preventivi nella cartella Fornitori", "Apri il sito del comune e trova gli orari dell'anagrafe"].map((s) => (
                  <button key={s} className="btn sm" onClick={() => setInput(s)}>{s}</button>
                ))}
              </div>
            </div>
          )}
          {items.map((it, idx) => (
            <ItemView key={`${it.kind}-${"id" in it ? it.id : idx}-${idx}`} it={it} onRespond={respond} onAction={sendAction} busy={!!runId} />
          ))}
        </div>
        <div className="composer">
          <div className="composer-box">
            <textarea
              id="composer"
              aria-label="Messaggio"
              rows={1}
              placeholder={active ? `Scrivi a ${agent?.name ?? "l'agente"}…` : "Crea una sessione per iniziare"}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void send(); } }}
            />
            {runId ? (
              <button className="btn danger" onClick={() => api.cancelRun(runId)}><Icon name="stop" size={15} /> Ferma</button>
            ) : (
              <button className="btn primary" onClick={() => void send()} disabled={!input.trim() || !active}><Icon name="send" size={15} /> Invia</button>
            )}
          </div>
          <div className="composer-meta">
            {profile && <span className="mono">{profile.runtime_id} · {profile.model}</span>}
            <span>Invio per mandare · Maiusc+Invio per andare a capo</span>
          </div>
        </div>
      </section>

      <Inspector runState={runState} agent={agent} profile={profile} pending={pendingApprovals} onRespond={respond} onClose={() => setShowInspector(false)} running={!!runId}
        session={active} onWorktreeGone={() => void sessions.reload()} />
    </div>
  );
}

function SessionItem({ s, agents, active, onClick }: { s: Session; agents: AgentProfile[]; active: boolean; onClick: () => void }) {
  const a = agents.find((x) => x.id === s.agent_id);
  return (
    <button className="session-item" aria-current={active} onClick={onClick}>
      <span className="t">{s.title}</span>
      <span className="m">
        <span className={`led ${s.last_status === "failed" ? "crit" : s.last_status === "running" ? "run" : s.last_status ? "ok" : "off"}`} style={{ width: 6, height: 6, boxShadow: "none" }} />
        {a?.name ?? s.agent_id} · {relTime(s.updated_at)}
        {s.worktree && <span title={s.worktree.branch} style={{ display: "inline-flex" }}><Icon name="branch" size={12} /></span>}
      </span>
    </button>
  );
}

function prettyArgs(args: string): string {
  try {
    return JSON.stringify(JSON.parse(args), null, 2);
  } catch {
    return args;
  }
}

function argSummary(args: string): string {
  try {
    const o = JSON.parse(args) as Record<string, unknown>;
    return Object.entries(o).map(([k, v]) => `${k}=${typeof v === "string" ? v : JSON.stringify(v)}`).join("  ");
  } catch {
    return args;
  }
}

const RISK_LABEL: Record<ApprovalRequest["risk"], string> = { low: "basso", medium: "medio", high: "alto" };

const toolIcon = (name: string) =>
  name.startsWith("browser") || name === "web_fetch" ? "globe" : name === "shell" ? "terminal" : name.startsWith("computer") ? "mouse" : name.startsWith("mcp__") ? "link" : "file";

function ItemView({ it, onRespond, onAction, busy }: {
  it: Item; onRespond: (r: ApprovalRequest, d: "allow" | "deny", remember?: boolean) => void; onAction: (a: A2uiAction) => void; busy: boolean;
}) {
  const [open, setOpen] = useState(false);
  switch (it.kind) {
    case "surface":
      return <div className="msg assistant"><A2uiSurface surface={it.surface} onAction={onAction} disabled={busy} /></div>;
    case "action":
      return (
        <div className="msg user">
          <div className="action-chip" title="Azione inviata dall'interfaccia">
            <Icon name="play" size={13} /> <b>{it.name}</b>
            {Object.keys(it.context).length > 0 && (
              <span className="mono">{Object.entries(it.context).map(([k, v]) => `${k}=${typeof v === "string" ? v : JSON.stringify(v)}`).join("  ")}</span>
            )}
          </div>
        </div>
      );
    case "user":
      return <div className="msg user"><div className="bubble">{it.text}</div></div>;
    case "text":
      return (
        <div className="msg assistant">
          <div className="bubble"><Markdown text={it.text + (it.streaming ? " ▍" : "")} /></div>
        </div>
      );
    case "reasoning":
      return (
        <div className="msg assistant">
          <details className="reasoning" open={it.streaming}>
            <summary>{it.streaming ? "Sta ragionando…" : "Ragionamento"}</summary>
            {it.text}
          </details>
        </div>
      );
    case "tool":
      // The surface itself is shown; keep the card only when the call failed.
      if (it.name === "render_ui" && !(it.result ?? "").startsWith("ERRORE")) return null;
      return (
        <div className="msg assistant">
          <div className="toolcard">
            <div className="toolcard-head" onClick={() => setOpen((v) => !v)} role="button" tabIndex={0} aria-expanded={open}
              onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && setOpen((v) => !v)}>
              <Icon name={toolIcon(it.name)} size={15} />
              <span className="name">{it.name}</span>
              <span className="args">{argSummary(it.args)}</span>
              {it.status === "running" ? <span className="led run" /> : it.status === "denied" ? <span className="pill crit">negato</span> : <Icon name="check" size={15} />}
            </div>
            {open && <pre>{prettyArgs(it.args)}</pre>}
            {it.result !== undefined && <pre>{it.result}</pre>}
          </div>
        </div>
      );
    case "approval":
      return (
        <div className="msg assistant">
          <div className="approval" role="alert">
            <div className="row">
              <Icon name="shield" />
              <b>Serve la tua conferma</b>
              <span className={`pill ${it.req.risk === "high" ? "crit" : it.req.risk === "medium" ? "warn" : "info"}`}>rischio {RISK_LABEL[it.req.risk]}</span>
            </div>
            <span>{it.req.reason}</span>
            <pre>{it.req.tool} {JSON.stringify(it.req.args, null, 2)}</pre>
            {it.resolved === undefined ? (
              <div className="row">
                <button className="btn primary" onClick={() => onRespond(it.req, "allow")}><Icon name="check" size={15} /> Consenti</button>
                <button className="btn" onClick={() => onRespond(it.req, "allow", true)}>Consenti sempre per questa sessione</button>
                <button className="btn danger" onClick={() => onRespond(it.req, "deny")}>Nega</button>
              </div>
            ) : (
              <span className={`pill ${it.resolved ? "ok" : "crit"}`}>{it.resolved ? "Consentito" : "Negato"}</span>
            )}
          </div>
        </div>
      );
    case "subagent":
      return (
        <div className="msg assistant">
          <div className="subagent">
            <Icon name="agent" size={15} />
            <span>Subagente <b>{it.name}</b></span>
            {it.result ? <span className="muted">→ {it.result}</span> : <span className="led run" />}
          </div>
        </div>
      );
    case "note":
      return <div className="msg assistant"><div className="loop-note"><Icon name="refresh" size={14} /> {it.text}</div></div>;
    case "error":
      return <div className="msg assistant"><span className="pill crit">{it.message}</span></div>;
  }
}

function Inspector({ runState, agent, profile, pending, onRespond, onClose, running, session, onWorktreeGone }: {
  runState: RunState; agent: AgentProfile | null; profile: InferenceProfile | null;
  pending: Extract<Item, { kind: "approval" }>[]; onRespond: (r: ApprovalRequest, d: "allow" | "deny") => void; onClose: () => void; running: boolean;
  session: Session | null; onWorktreeGone: () => void;
}) {
  const u = runState.usage;
  const ctxPct = u?.context_used && u.context_max ? u.context_used / u.context_max : 0;
  const todos = useMemo(() => runState.todos ?? [], [runState.todos]);
  return (
    <aside className="inspector" aria-label="Pannello del run">
      <div className="row">
        <h3 className="grow">Run</h3>
        {running ? <span className="pill run"><span className="led run" style={{ width: 7, height: 7, boxShadow: "none" }} />In esecuzione</span> : <span className="faint" style={{ fontSize: "var(--fs-xs)" }}>inattivo</span>}
        <button className="icon-btn only-narrow" onClick={onClose} aria-label="Chiudi pannello"><Icon name="close" /></button>
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
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h3>Contesto</h3>
          <span className="mono faint" style={{ fontSize: "var(--fs-xs)" }}>{u?.context_used ?? 0} / {u?.context_max ?? profile?.params.context ?? "—"}</span>
        </div>
        <Meter value={u?.context_used ?? 0} max={u?.context_max ?? 1} tone={ctxPct > 0.85 ? "crit" : ctxPct > 0.7 ? "warn" : undefined} />
        <span className="faint" style={{ fontSize: "var(--fs-xs)" }}>Compattazione automatica all'80%</span>
      </div>
      <div className="stack">
        <h3>Piano</h3>
        {todos.length === 0 && <span className="faint" style={{ fontSize: "var(--fs-sm)" }}>L'agente pubblica qui i suoi passi.</span>}
        {todos.map((t, i) => (
          <div key={i} className={`todo ${t.status}`}>
            {t.status === "completed" ? <Icon name="check" size={15} /> : <span className={`led ${t.status === "in_progress" ? "run" : "off"}`} style={{ marginTop: 6 }} />}
            <span>{t.content}</span>
          </div>
        ))}
      </div>
      {session?.worktree && <WorktreePanel key={session.id} session={session} running={running} onGone={onWorktreeGone} />}
      {pending.length > 0 && (
        <div className="stack">
          <h3>Conferme in attesa</h3>
          {pending.map((p) => (
            <div key={p.req.id} className="approval" style={{ padding: 10 }}>
              <span className="mono" style={{ fontSize: "var(--fs-xs)" }}>{p.req.tool}</span>
              <div className="row">
                <button className="btn primary sm" onClick={() => onRespond(p.req, "allow")}>Consenti</button>
                <button className="btn danger sm" onClick={() => onRespond(p.req, "deny")}>Nega</button>
              </div>
            </div>
          ))}
        </div>
      )}
    </aside>
  );
}

const FILE_STATUS: Record<string, string> = { A: "aggiunto", M: "modificato", D: "eliminato", R: "rinominato", C: "copiato", T: "tipo cambiato" };

/** Changes of an isolated session: review the diff, apply it to the workspace or throw it away. */
function WorktreePanel({ session, running, onGone }: { session: Session; running: boolean; onGone: () => void }) {
  const { api, toast } = useApp();
  const [st, setSt] = useState<WorktreeStatus | null>(null);
  const [showDiff, setShowDiff] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const wt = session.worktree!;

  const load = useCallback(async () => {
    try {
      setSt(await api.worktreeStatus(session.id));
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  }, [api, session.id, toast]);
  // Refresh when the session changes and whenever a run ends.
  useEffect(() => {
    if (!running) void load();
  }, [load, running]);

  const apply = async () => {
    setBusy(true);
    try {
      setSt(await api.worktreeApply(session.id));
      setShowDiff(false);
      toast("Modifiche applicate al workspace (non ancora committate)");
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    } finally {
      setBusy(false);
    }
  };
  const discard = async () => {
    if (!confirmDiscard) {
      setConfirmDiscard(true);
      return;
    }
    setBusy(true);
    try {
      await api.worktreeDiscard(session.id);
      toast(`Worktree ${wt.branch} eliminato`);
      onGone();
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    } finally {
      setBusy(false);
      setConfirmDiscard(false);
    }
  };

  const files = st?.files ?? [];
  return (
    <div className="stack worktree">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h3>Worktree</h3>
        <span className="pill neutral mono"><Icon name="branch" size={13} /> {wt.branch}</span>
      </div>
      <span className="faint mono wt-path" title={wt.path}>{wt.path}</span>
      {st && files.length === 0 && <span className="faint" style={{ fontSize: "var(--fs-sm)" }}>Nessuna modifica da applicare.</span>}
      {files.length > 0 && (
        <ul className="changed" aria-label="File modificati">
          {files.map((f) => (
            <li key={f.path}>
              <span className={`fs fs-${f.status}`} title={FILE_STATUS[f.status] ?? f.status}>{f.status}</span>
              <span className="mono">{f.path}</span>
            </li>
          ))}
        </ul>
      )}
      <div className="row">
        <button className="btn sm" disabled={files.length === 0} onClick={() => setShowDiff((v) => !v)} aria-expanded={showDiff}>
          {showDiff ? "Nascondi differenze" : "Differenze"}
        </button>
        <button className="btn primary sm" disabled={busy || running || files.length === 0} onClick={() => void apply()}>
          <Icon name="check" size={14} /> Applica al workspace
        </button>
        <button className="btn danger sm" disabled={busy || running} onClick={() => void discard()} onBlur={() => setConfirmDiscard(false)}>
          {confirmDiscard ? "Conferma: elimina" : "Scarta"}
        </button>
      </div>
      {running && <span className="faint" style={{ fontSize: "var(--fs-xs)" }}>Applica e scarta tornano disponibili alla fine del run.</span>}
      {showDiff && st && <Diff text={st.diff} truncated={st.truncated} />}
    </div>
  );
}

function Diff({ text, truncated }: { text: string; truncated: boolean }) {
  const lines = text.split("\n");
  const cls = (l: string) =>
    l.startsWith("+++") || l.startsWith("---") || l.startsWith("diff ") || l.startsWith("new file") || l.startsWith("index ")
      ? "meta"
      : l.startsWith("+") ? "add" : l.startsWith("-") ? "del" : l.startsWith("@@") ? "hunk" : undefined;
  return (
    <pre className="diff" aria-label="Differenze">
      <code>
        {lines.map((l, i) => <span key={i} className={cls(l)}>{l || " "}</span>)}
        {truncated && <span className="meta">… differenze troncate</span>}
      </code>
    </pre>
  );
}
