import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { navigate, useParams } from "../../app/router";
import { useApi } from "../../shared/api/context";
import { errorMessage } from "../../shared/lib/errors";
import { lastChatSession, rememberChatSession } from "../../shared/lib/navMemory";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import { Icon, IconButton, Led, Pill } from "../../shared/ui";
import { useAgents, useProfiles } from "../agents/api";
import { useRespondApproval } from "../approvals/api";
import { actionText, type A2uiAction } from "./a2ui";
import { useCancelRun, useCreateSession, useSessions, useStartRun } from "./api";
import { Composer } from "./components/Composer";
import { Inspector } from "./components/Inspector";
import { ItemView } from "./components/ItemView";
import { SessionList } from "./components/SessionList";
import { getDraft, setDraft } from "./model/drafts";
import { historyToItems } from "./model/history";
import type { ApprovalItem, Item } from "./model/items";
import { reduce } from "./model/reduce";
import type { AgEvent, ApprovalRequest, RunState } from "../../shared/api/types";

const SUGGESTIONS = ["Riordina i PDF in Downloads per anno e crea un indice", "Confronta i preventivi nella cartella Fornitori", "Apri il sito del comune e trova gli orari dell'anagrafe"];

export function ChatView() {
  const api = useApi();
  const qc = useQueryClient();
  const sessions = useSessions();
  const agents = useAgents();
  const profiles = useProfiles();
  const createSession = useCreateSession();
  const startRun = useStartRun();
  const cancelRun = useCancelRun();
  const respondApproval = useRespondApproval({ silent: true });

  // The active session lives in the URL (#/chat/:sessionId): back/forward, links and view switches keep it.
  const activeId = useParams().sessionId ?? null;
  const [items, setItems] = useState<Item[]>([]);
  const [runId, setRunId] = useState<string | null>(null);
  const [runState, setRunState] = useState<RunState>({});
  const [input, setInputState] = useState(() => getDraft(activeId));
  const [showInspector, setShowInspector] = useState(false);
  const [showSessions, setShowSessions] = useState(false);
  const [newAgent, setNewAgent] = useState<string>("operator");
  const [isolated, setIsolated] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const unsub = useRef<(() => void) | null>(null);

  const list = sessions.data;
  const active = list?.find((s) => s.id === activeId) ?? null;
  const agent = agents.data?.find((a) => a.id === active?.agent_id) ?? null;
  const profile = profiles.data?.find((p) => p.id === agent?.inference_profile) ?? null;

  // No session in the URL (or a stale one): open the last used one, else the most recent.
  useEffect(() => {
    if (!list || list.length === 0) return;
    if (activeId && list.some((s) => s.id === activeId)) return;
    const last = lastChatSession();
    const target = list.find((s) => s.id === last) ?? list[0];
    navigate(`/chat/${encodeURIComponent(target.id)}`, { replace: true });
  }, [list, activeId]);

  useEffect(() => {
    if (active) rememberChatSession(active.id);
  }, [active]);

  // Each session keeps its own unsent draft.
  useEffect(() => {
    setInputState(getDraft(activeId));
  }, [activeId]);
  const setInput = (v: string) => {
    setInputState(v);
    setDraft(activeId, v);
  };

  const refreshSessions = useCallback(() => void qc.invalidateQueries({ queryKey: qk.sessions.list() }), [qc]);

  const onEventRef = useRef<(e: AgEvent) => void>(() => {});
  const onEvent = useCallback((e: AgEvent) => {
    if (e.type === "STATE_SNAPSHOT") setRunState(e.snapshot);
    if (e.type === "RUN_FINISHED" || e.type === "RUN_ERROR") {
      setRunId(null);
      refreshSessions();
    }
    setItems((prev) => reduce(prev, e));
  }, [refreshSessions]);
  onEventRef.current = onEvent;

  useEffect(() => {
    if (!activeId) {
      setItems([]);
      return;
    }
    unsub.current?.();
    setRunId(null);
    setRunState({});
    let alive = true;
    qc.fetchQuery({ queryKey: qk.sessions.messages(activeId), queryFn: () => api.messages(activeId), staleTime: 0 })
      .then(async (m) => {
        if (!alive) return;
        // A run may still be going on (started elsewhere, or before switching session): re-attach.
        const running = await api.activeRun(activeId).catch(() => null);
        if (!alive) return;
        // History already contains the user prompt of the running turn; events rebuild the rest.
        setItems(historyToItems(m));
        if (running) {
          setRunId(running);
          unsub.current = api.subscribeRun(running, (e) => onEventRef.current(e));
        }
      })
      .catch((e) => toast(errorMessage(e), "crit"));
    return () => {
      alive = false;
    };
  }, [activeId, api, qc]);

  // Follow the conversation, but jump (not animate) so streaming does not queue smooth scrolls.
  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [items]);

  useEffect(() => () => unsub.current?.(), []);

  const attach = useCallback((run_id: string) => {
    setRunId(run_id);
    unsub.current?.();
    unsub.current = api.subscribeRun(run_id, (e) => onEventRef.current(e));
  }, [api]);

  const send = () => {
    const text = input.trim();
    if (!text || !activeId || runId) return;
    setInput("");
    setItems((prev) => [...prev, { kind: "user", id: `u-${Date.now()}`, text }]);
    startRun.mutate({ sessionId: activeId, input: text }, {
      onSuccess: ({ run_id }) => attach(run_id),
      onError: (e) => toast(`Impossibile avviare il run: ${errorMessage(e)}`, "crit"),
    });
  };

  const actionRef = useRef<(a: A2uiAction) => void>(() => {});
  actionRef.current = (a) => {
    if (!activeId || runId) return;
    setItems((prev) => [...prev, { kind: "action", id: `a-${Date.now()}`, name: a.name, context: a.context }]);
    startRun.mutate({ sessionId: activeId, input: actionText(a), kind: "a2ui.action" }, {
      onSuccess: ({ run_id }) => attach(run_id),
      onError: (e) => toast(`Impossibile inviare l'azione: ${errorMessage(e)}`, "crit"),
    });
  };
  const onAction = useCallback((a: A2uiAction) => actionRef.current(a), []);

  const newSession = () => {
    createSession.mutate({ agent_id: newAgent, isolation: isolated ? "worktree" : "none" }, {
      onSuccess: (s) => {
        navigate(`/chat/${encodeURIComponent(s.id)}`);
        setShowSessions(false);
      },
    });
  };

  const respondRef = useRef<(r: ApprovalRequest, d: "allow" | "deny", remember?: boolean) => void>(() => {});
  respondRef.current = (req, decision, remember = false) => {
    respondApproval.mutate({ request: req, decision, remember }, {
      onSuccess: () => setItems((prev) => prev.map((i) => (i.kind === "approval" && i.req.id === req.id ? { ...i, resolved: decision === "allow" } : i))),
    });
  };
  const respond = useCallback((r: ApprovalRequest, d: "allow" | "deny", remember?: boolean) => respondRef.current(r, d, remember), []);

  const pendingApprovals = items.filter((i) => i.kind === "approval" && i.resolved === undefined) as ApprovalItem[];

  return (
    <div className={`chat ${showInspector ? "show-inspector" : ""} ${showSessions ? "show-sessions" : ""}`}>
      <SessionList
        sessions={list ?? []} agents={agents.data ?? []} activeId={activeId} newAgent={newAgent} onNewAgent={setNewAgent}
        isolated={isolated} onIsolated={setIsolated} onCreate={newSession}
        onSelect={(id) => { navigate(`/chat/${encodeURIComponent(id)}`); setShowSessions(false); }}
        onClose={() => setShowSessions(false)} showClose={showSessions}
      />

      <section className="convo" aria-label="Conversazione">
        <div className="convo-head">
          <IconButton className="only-narrow" aria-label="Sessioni" icon="list" onClick={() => setShowSessions(true)} />
          <h2 className="grow">{active?.title ?? "Workspace"}</h2>
          {agent && <Pill><Icon name="agent" size={13} /> {agent.name}</Pill>}
          {agent && <Pill tone="info" title="Modalità di approvazione"><Icon name="shield" size={13} /> {agent.mode}</Pill>}
          {active?.worktree && (
            <button type="button" className="pill neutral mono pill-btn" onClick={() => setShowInspector(true)} title={`Worktree isolato: ${active.worktree.path}`}>
              <Icon name="branch" size={13} /> {active.worktree.branch}
            </button>
          )}
          {runId ? <Pill tone="run"><Led tone="run" small />In esecuzione</Pill> : null}
          <IconButton aria-label="Pannello run" icon="panel" onClick={() => setShowInspector((v) => !v)} />
        </div>
        <div className="convo-scroll" ref={scrollRef}>
          {items.length === 0 && (
            <div className="empty chat-empty">
              <h2>Cosa facciamo, in locale?</h2>
              <p>L'agente lavora con i modelli di questo dispositivo e chiede conferma prima di modificare file o eseguire comandi.</p>
              <div className="row">
                {SUGGESTIONS.map((s) => <button key={s} type="button" className="btn sm" onClick={() => setInput(s)}>{s}</button>)}
              </div>
            </div>
          )}
          {items.map((it, idx) => (
            <ItemView key={`${it.kind}-${"id" in it ? it.id : idx}-${idx}`} it={it} onRespond={respond} onAction={onAction} busy={!!runId} />
          ))}
        </div>
        <Composer
          value={input} onChange={setInput} onSend={send} onStop={() => runId && cancelRun.mutate(runId)}
          running={!!runId} disabled={!active} profile={profile}
          placeholder={active ? `Scrivi a ${agent?.name ?? "l'agente"}…` : "Crea una sessione per iniziare"}
        />
      </section>

      <Inspector runState={runState} agent={agent} profile={profile} pending={pendingApprovals} onRespond={respond} onClose={() => setShowInspector(false)} running={!!runId} session={active} />
    </div>
  );
}
