import { useStoredUiMode } from "../../shared/lib/uiMode";
import { useMcpPrompts } from "../mcp/api";
import { McpPromptPicker } from "../mcp/McpPromptPicker";
import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Link, navigate, useParams } from "../../app/router";
import { lastChatSession, rememberChatSession } from "../../shared/lib/navMemory";
import { defaultAgentId } from "../../shared/lib/agents";
import { forgetSessionRuns } from "../../shared/lib/runIndex";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import { useApi } from "../../shared/api/context";
import { useSystem } from "../../shared/query/system";
import type { Session } from "../../shared/api/types";
import { Button, Drawer, EmptyState, Icon, ModalShell, Sheet } from "../../shared/ui";
import { useMediaQuery } from "../../shared/lib/useMediaQuery";
import { useAgents, useProfiles } from "../agents/api";
import { useModels, useRecommend } from "../models/api";
import { autoProfileFromAdvice } from "../models/model/adviser";
import { canReason } from "../../shared/lib/thinking";
import { useLessons } from "../learning/api";
import { RememberCard } from "../learning/components/RememberCard";
import { memoryPendingFor, pendingFor } from "../learning/model/lessons";
import { useTeams } from "../teams/api";
import { teamAsAgent, teamIdOf } from "../teams/model/teams";
import { useConfig } from "../settings/api";
import { useCreateSession, useSessions } from "./api";
import { ChatEmpty } from "./components/ChatEmpty";
import { ChatHeader } from "./components/ChatHeader";
import { Composer } from "./components/Composer";
import { useAttachments } from "./useAttachments";
import { ConversationLog, type ConversationLogHandle } from "./components/ConversationLog";
import { InspectorBody } from "./components/Inspector";
import { SessionList, type NewSessionInput } from "./components/SessionList";
import { setDraft } from "./model/drafts";
import { forgetThinkingChoice } from "./model/thinkingChoice";
import { forgetSessionImages } from "./model/imageCache";
import { describeRunError, lastRunNotice } from "./model/runNotice";
import { useConversation } from "./useConversation";
import { useFeedback } from "./useFeedback";
import { useThinking } from "./useThinking";
import { useVoice } from "./useVoice";
import { COMPACT_SESSIONS_QUERY, useDraft, useInspectorMode, useInspectorOpen } from "./useLayout";
import "./chat.css";

import { t as tr, t } from "../../shared/i18n";
const chatPath = (id: string) => `/chat/${encodeURIComponent(id)}`;

/** Workspace: sessions on the left, the conversation in the middle, the run panel on the right. */
export function ChatView() {
  const sessions = useSessions();
  const agents = useAgents();
  const teams = useTeams();
  const profiles = useProfiles();
  const config = useConfig();
  const createSession = useCreateSession();
  // Mobile and Mac App Store: bring back the agent's native browser after the user closes it.
  const api = useApi();
  const system = useSystem();
  const browserButton = api.openBrowser && (system.data?.device.os === "ios" || system.data?.device.os === "android"
    || (system.data?.device.os === "macos" && system.data.sandboxed))
    ? () => { void api.openBrowser?.().catch((e: unknown) => toast(e instanceof Error ? e.message : tr("chat.non_riesco_ad_aprire_il"), "crit")); }
    : undefined;

  // The open session lives in the URL (#/chat/:sessionId): back/forward, links and view switches keep it.
  const urlId = useParams().sessionId ?? null;
  const list = sessions.data;
  const active = list?.find((s) => s.id === urlId) ?? null;
  const openId = active?.id ?? null;
  // A team session (`team:<id>`) shows the profile the engine runs it with, under the team's name.
  const sessionTeamId = active ? teamIdOf(active.agent_id) : null;
  const sessionTeam = sessionTeamId ? teams.data?.find((t) => t.id === sessionTeamId) : undefined;
  const agent = sessionTeamId
    ? (sessionTeam ? teamAsAgent(sessionTeam, agents.data ?? []) : null)
    : agents.data?.find((a) => a.id === active?.agent_id) ?? null;
  // An `auto:*` profile is not stored: show what the adviser resolves it to.
  const recommend = useRecommend();
  const profile = profiles.data?.find((p) => p.id === agent?.inference_profile)
    ?? (agent ? autoProfileFromAdvice(agent.inference_profile, recommend.data) : null);
  const defaultAgent = defaultAgentId(agents.data ?? [], config.data?.effective["agent.default"]);

  // No session in the URL (or a stale one): open the last used one, else the most recent.
  useEffect(() => {
    if (!list || list.length === 0) return;
    if (urlId && list.some((s) => s.id === urlId)) return;
    const target = list.find((s) => s.id === lastChatSession()) ?? list[0];
    navigate(chatPath(target.id), { replace: true });
  }, [list, urlId]);
  useEffect(() => {
    if (openId) rememberChatSession(openId);
  }, [openId]);

  // The open session was deleted from another client (404): forget it, go back to the list and say so.
  const qc = useQueryClient();
  const gone = useCallback(
    (id: string) => {
      forgetSessionImages(id);
      forgetSessionRuns(id);
      setDraft(id, "");
      forgetThinkingChoice(id);
      if (lastChatSession() === id) rememberChatSession(null);
      // Out of the list at once, so the redirect below never picks it again.
      qc.setQueryData<Session[]>(qk.sessions.list(), (l) => l?.filter((x) => x.id !== id));
      void qc.invalidateQueries({ queryKey: qk.sessions.all() });
      toast(tr("chat.questa_sessione_non_esiste_piu"));
      navigate("/chat", { replace: true });
    },
    [qc],
  );
  // "Ragionamento": offered only when the model of the session's profile can reason; the choice goes with every run.
  const models = useModels();
  const reasons = canReason(models.data?.find((m) => m.id === profile?.model));
  const [thinking, setThinking] = useThinking(openId, profile?.params.thinking);
  const conv = useConversation(openId, { onGone: gone, thinking: reasons ? thinking : undefined });
  const feedback = useFeedback(openId, conv.items, conv.running);
  // After a thumbs down the agent may have proposed a lesson: point to it.
  const pendingLessons = useLessons("pending").data;
  // A memory lesson is a small card in the conversation; a skill lesson still points to the Panoramica.
  const memoryToConfirm = memoryPendingFor(pendingLessons, openId);
  const lessonToReview = memoryToConfirm ? undefined : pendingFor(pendingLessons, openId);
  const [input, setInput] = useDraft(openId);
  // «Prompt MCP»: developer mode, and only when a connected server has prompts.
  const developer = useStoredUiMode() === "developer";
  const mcpPrompts = useMcpPrompts(developer);
  const [promptPicker, setPromptPicker] = useState(false);
  const [focusTick, setFocusTick] = useState(0);
  const [showSessions, setShowSessions] = useState(false);
  const compactSessions = useMediaQuery(COMPACT_SESSIONS_QUERY);
  const mode = useInspectorMode();
  const panel = useInspectorOpen(mode);
  const log = useRef<ConversationLogHandle>(null);

  // Say in a polite live region when a reply starts and ends; the log itself stays silent while it streams.
  const [announce, setAnnounce] = useState("");
  const wasRunning = useRef(false);
  // Leaving a conversation is not the end of its run.
  useEffect(() => {
    wasRunning.current = false;
    setAnnounce("");
  }, [openId]);
  useEffect(() => {
    if (conv.running && !wasRunning.current) setAnnounce("Risposta in corso");
    if (!conv.running && wasRunning.current) {
      const last = conv.items[conv.items.length - 1];
      setAnnounce(last?.kind === "error" ? describeRunError(last.code).title : "Risposta completata");
    }
    wasRunning.current = conv.running;
  }, [conv.running, conv.items]);

  const attachments = useAttachments();
  // Files belong to the conversation they were chosen in.
  const { clear: clearAttachments } = attachments;
  useEffect(() => clearAttachments(), [openId, clearAttachments]);

  const send = () => {
    const text = input.trim();
    const files = attachments.files;
    if ((!text && files.length === 0) || !openId || conv.running) return;
    setInput("");
    attachments.clear();
    log.current?.scrollToBottom();
    // A message that could not start gives its text and its files back.
    void conv.send(text, () => {
      setInput(text);
      attachments.restore(files);
    }, files);
  };

  const create = (input: NewSessionInput) =>
    createSession.mutate(input, {
      onSuccess: (s) => {
        navigate(chatPath(s.id));
        setShowSessions(false);
      },
    });
  const onDeleted = (id: string) => {
    forgetSessionImages(id);
    forgetSessionRuns(id);
    setDraft(id, "");
    forgetThinkingChoice(id);
    if (id === openId) {
      rememberChatSession(null);
      navigate("/chat", { replace: true });
    }
  };
  const sessionList = (onClose?: () => void) => (
    <SessionList
      sessions={sessions} agents={agents.data ?? []} teams={teams.data ?? []} defaultAgent={defaultAgent} activeId={openId}
      creating={createSession.isPending} onCreate={create} onDeleted={onDeleted} onClose={onClose}
      onSelect={(id) => {
        navigate(chatPath(id));
        setShowSessions(false);
      }}
    />
  );

  const noSessions = !!list && list.length === 0;
  // Until there is a session to show: loading while the list loads (or the redirect to one runs), the list's own error if it failed.
  const listFailed = sessions.isError && !list;
  const logState = listFailed
    ? { status: "error" as const, error: sessions.error, title: tr("chat.impossibile_caricare_le_sessioni") }
    : sessions.isPending || (!!list?.length && !openId) ? { status: "loading" as const } : conv.history;
  const voice = useVoice({
    sessionId: openId, ready: !!openId && logState.status === "ready", running: conv.running, items: conv.items, pending: conv.pending,
    send: (text, onRejected) => {
      log.current?.scrollToBottom();
      void conv.send(text, onRejected);
    },
    insertText: (text) => setInput(input.trim() ? `${input.trimEnd()} ${text}` : text),
  });

  const notice = lastRunNotice(active?.last_status, conv.items, conv.running);
  const compactAt = Number(config.data?.effective["context.compact_at"]) || 0.8;
  const inspector = (
    <InspectorBody
      runState={conv.runState} agent={agent} profile={profile} session={active} pending={conv.pending} running={conv.running}
      respondingId={conv.respondingId} compactAt={compactAt} onRespond={conv.respond}
    />
  );

  return (
    <div className="ws">
      <h1 className="sr-only">{tr("chat.workspace")}</h1>
      {!compactSessions && <aside className="ws-sessions" aria-label={tr("chat.sessioni")}>{sessionList()}</aside>}

      <section className="ws-main" aria-label={tr("chat.workspace")}>
        <ChatHeader
          session={active} agent={agent} profile={profile} running={conv.running}
          sessionsButton={compactSessions ? { onClick: () => setShowSessions(true), expanded: showSessions } : undefined}
          inspectorOpen={panel.open} onToggleInspector={panel.toggle} onOpenWorktree={panel.show} onOpenBrowser={browserButton}
        />
        {noSessions ? (
          <div className="ws-nosession">
            <EmptyState
              icon="chat" title={tr("chat.nessuna_sessione")}
              description={tr("chat.crea_la_prima_sessione_per")}
              action={<Button variant="primary" icon="plus" disabled={!defaultAgent || createSession.isPending} onClick={() => create({ agent_id: defaultAgent, isolation: "none" })}>{tr("chat.nuova_sessione")}</Button>}
            />
          </div>
        ) : (
          <ConversationLog
            ref={log} items={conv.items} state={logState} onRetry={listFailed ? () => void sessions.refetch() : conv.reload} busy={conv.running} respondingId={conv.respondingId}
            onRespond={conv.respond} onAction={conv.sendAction} notice={notice} sessionKey={openId} feedback={feedback}
            footer={memoryToConfirm && !conv.running ? <RememberCard key={memoryToConfirm.id} lesson={memoryToConfirm} /> : undefined}
            empty={<ChatEmpty onPick={(text) => { setInput(text); setFocusTick((n) => n + 1); }} />}
          />
        )}
        {lessonToReview && (
          <div className="ws-note ws-lesson" role="status">
            <Icon name="sparkle" size={18} />
            <div className="ws-note-body">
              <b>{tr("chat.ho_proposto_una_lezione_da")}</b>
              <span className="ws-note-hint">{tr("chat.non_viene_scritta_finche_non", { title: lessonToReview.title })}</span>
              <Link to="/" className="ws-note-link">{t("chat.rivedila_nella_panoramica")}{" "}<Icon name="forward" size={12} /></Link>
            </div>
          </div>
        )}
        <Composer
          attachments={{ items: attachments.files, onAdd: attachments.add, onRemove: attachments.remove }}
          value={input} onChange={setInput} onSend={send} onStop={conv.stop} running={conv.running} disabled={!openId || logState.status !== "ready"}
          onMcpPrompt={developer && (mcpPrompts.data?.length ?? 0) > 0 ? () => setPromptPicker(true) : undefined}
          focusKey={`${openId}:${focusTick}`} voice={voice} thinking={reasons ? { value: thinking, onChange: setThinking } : undefined}
          modelLabel={profile ? `${profile.runtime_id} · ${profile.model || "nessun modello"}` : undefined}
          placeholder={!openId ? tr("chat.crea_una_sessione_per_iniziare") : logState.status === "loading" ? tr("chat.carico_la_conversazione") : tr("chat.scrivi_a", { p: agent?.name ?? "l'agente" })}
        />
        {promptPicker && mcpPrompts.data && (
          <McpPromptPicker
            prompts={mcpPrompts.data} onClose={() => setPromptPicker(false)}
            onInsert={(text) => { setInput(input.trim() ? `${input.trimEnd()}\n\n${text}` : text); setFocusTick((n) => n + 1); }}
          />
        )}
        <p className="sr-only" role="status" aria-live="polite">{announce}</p>
      </section>

      {mode === "inline" && panel.open && <aside className="ws-inspector" aria-label={tr("chat.pannello_del_run")}>{inspector}</aside>}
      {mode === "drawer" && panel.open && <Drawer title={tr("chat.pannello_del_run")} onClose={panel.close}>{inspector}</Drawer>}
      {mode === "sheet" && panel.open && <Sheet title={tr("chat.pannello_del_run")} onClose={panel.close}>{inspector}</Sheet>}
      {compactSessions && showSessions && (
        <ModalShell variant="drawer" title={tr("chat.sessioni")} className="ws-drawer-left" onClose={() => setShowSessions(false)}>
          {sessionList(() => setShowSessions(false))}
        </ModalShell>
      )}
    </div>
  );
}
