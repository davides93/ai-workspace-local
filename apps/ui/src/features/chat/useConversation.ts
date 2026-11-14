import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { errorMessage, isNotFound } from "../../shared/lib/errors";
import { rememberRun } from "../../shared/lib/runIndex";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import type { AgEvent, ApprovalRequest, AttachmentUpload, RunState, ThinkingMode } from "../../shared/api/types";
import { useRespondApproval } from "../approvals/api";
import { actionText, type A2uiAction } from "./a2ui";
import { useCancelRun, useStartRun } from "./api";
import { historyToItems } from "./model/history";
import { imagesFor, rememberImage, rememberSessionImages } from "./model/imageCache";
import { HANDOFF, type ApprovalItem, type Item } from "./model/items";
import { localAttachments, toUploads, type PendingFile } from "./model/pending";
import { parseImageEvent, reduce } from "./model/reduce";

import { t } from "../../shared/i18n";
export type HistoryState = { status: "loading" | "ready" | "error"; error?: unknown; title?: string };

/**
 * State of the open conversation: the rows (stored history first, then the live AG-UI events of the
 * run), the run that is going on, its state snapshot, and the actions on it. Opening a session
 * loads its history and re-attaches to a run that is still going; switching session drops the
 * old subscription (the run keeps going on the backend).
 *
 * `onGone` runs when the backend answers 404 for the open session (it was deleted from another client):
 * loading its history, or sending to it. The view then leaves the conversation.
 */
export function useConversation(sessionId: string | null, opts: { onGone?: (sessionId: string) => void; /** Reasoning for the next runs of this conversation (the "Ragionamento" selector). */ thinking?: ThinkingMode; /** Sees every event of the open conversation's runs (the Insegna page refreshes what it shows). */ onEvent?: (e: AgEvent) => void } = {}) {
  const api = useApi();
  const qc = useQueryClient();
  const startRun = useStartRun();
  const cancelRun = useCancelRun();
  const respondApproval = useRespondApproval({ silent: true });

  const [items, setItems] = useState<Item[]>([]);
  const [runId, setRunId] = useState<string | null>(null);
  const [runState, setRunState] = useState<RunState>({});
  const [history, setHistory] = useState<HistoryState>({ status: "loading" });
  const [reloadTick, setReloadTick] = useState(0);
  const unsub = useRef<(() => void) | null>(null);
  // The run asked to hand the conversation over: the engine starts the next agent's run once this one is stored.
  const handoffSeen = useRef(false);
  const follow = useRef<((forSession: string) => Promise<void>) | null>(null);
  const current = useRef(sessionId);
  current.current = sessionId;
  const onGone = useRef(opts.onGone);
  onGone.current = opts.onGone;
  const thinking = useRef(opts.thinking);
  thinking.current = opts.thinking;
  const onEvent = useRef(opts.onEvent);
  onEvent.current = opts.onEvent;

  const handle = useCallback(
    (forSession: string, e: AgEvent) => {
      // An event of a subscription that was dropped while it was being delivered.
      if (current.current !== forSession) return;
      onEvent.current?.(e);
      if (e.type === "RUN_STARTED") rememberRun(e.runId, forSession);
      // A member of a team sends a snapshot with only the shared state: merge, so the plan and usage stay.
      if (e.type === "STATE_SNAPSHOT") setRunState((prev) => ({ ...prev, ...e.snapshot }));
      if (e.type === "CUSTOM" && e.name === HANDOFF) handoffSeen.current = true;
      if (e.type === "CUSTOM" && e.name === "aiw.image") {
        const img = parseImageEvent(e.value);
        if (img) rememberImage(forSession, img.toolCallId, img.image);
      }
      if (e.type === "RUN_FINISHED" || e.type === "RUN_ERROR") {
        // The run is over: nothing more is expected from it (a carrier that keeps the stream open must not feed a stopped run).
        unsub.current?.();
        unsub.current = null;
        setRunId(null);
        void qc.invalidateQueries({ queryKey: qk.sessions.all() });
        void qc.invalidateQueries({ queryKey: qk.approvals() });
        if (e.type === "RUN_FINISHED" && handoffSeen.current) void follow.current?.(forSession);
      }
      setItems((prev) => reduce(prev, e));
    },
    [qc],
  );

  const attach = useCallback(
    (id: string, forSession: string) => {
      unsub.current?.();
      rememberRun(id, forSession);
      setRunId(id);
      unsub.current = api.subscribeRun(id, (e) => handle(forSession, e));
    },
    [api, handle],
  );

  /** After a handoff: waits for the run of the agent the conversation moved to, and follows it. */
  const followHandoff = useCallback(
    async (forSession: string) => {
      handoffSeen.current = false;
      for (let i = 0; i < 24 && current.current === forSession; i++) {
        await new Promise((r) => setTimeout(r, 250));
        const next = await api.activeRun(forSession).catch(() => null);
        if (current.current !== forSession) return;
        if (next) {
          void qc.invalidateQueries({ queryKey: qk.sessions.all() });
          return attach(next, forSession);
        }
      }
      void qc.invalidateQueries({ queryKey: qk.sessions.all() });
      setReloadTick((n) => n + 1);
    },
    [api, qc, attach],
  );
  follow.current = followHandoff;

  useEffect(() => {
    handoffSeen.current = false;
    unsub.current?.();
    unsub.current = null;
    setRunId(null);
    setRunState({});
    setItems([]);
    if (!sessionId) {
      setHistory({ status: "ready" });
      return;
    }
    setHistory({ status: "loading" });
    let alive = true;
    (async () => {
      try {
        const msgs = await qc.fetchQuery({ queryKey: qk.sessions.messages(sessionId), queryFn: () => api.messages(sessionId), staleTime: 0 });
        // A run may still be going on (started elsewhere, or before switching session): re-attach.
        // The screenshots of earlier runs come from the stored run events. An old server, a 404 or a network
        // error only means the cards show no image: it must not stop the conversation from opening.
        const [running, stored] = await Promise.all([api.activeRun(sessionId).catch(() => null), Promise.resolve().then(() => api.sessionImages(sessionId)).catch(() => [])]);
        if (!alive) return;
        rememberSessionImages(sessionId, stored);
        // The history already holds the prompt of the running turn; the events rebuild the rest.
        setItems(historyToItems(msgs, { images: imagesFor(sessionId) }));
        setHistory({ status: "ready" });
        if (running) attach(running, sessionId);
      } catch (e) {
        if (!alive) return;
        // Deleted from another client: nothing to retry, the view moves away (and says why).
        if (isNotFound(e) && onGone.current) return void onGone.current(sessionId);
        setHistory({ status: "error", error: e });
      }
    })();
    return () => {
      alive = false;
    };
  }, [sessionId, api, qc, attach, reloadTick]);

  useEffect(() => () => unsub.current?.(), []);

  // The actions below keep a stable identity (rows are memoized) and read the latest state from here.
  const latest = useRef({ sessionId, runId, startRun, cancelRun, respondApproval, attach });
  latest.current = { sessionId, runId, startRun, cancelRun, respondApproval, attach };

  /**
   * Sends a message, with the files chosen for it. `onRejected` runs when the run could not start
   * (to give the text and the files back).
   */
  const send = useCallback(async (text: string, onRejected?: () => void, files: PendingFile[] = []) => {
    const { sessionId: sid, runId: rid, startRun: start, attach: join } = latest.current;
    if (!sid || rid) return;
    let uploads: AttachmentUpload[] = [];
    if (files.length > 0) {
      try {
        uploads = await toUploads(files);
      } catch (e) {
        onRejected?.();
        return void toast(t("chat.non_riesco_a_leggere_gli", { error: errorMessage(e) }), "crit");
      }
      // The conversation changed while the files were read.
      if (current.current !== sid) return void onRejected?.();
    }
    const tempId = `u-${Date.now()}`;
    setItems((prev) => [...prev, { kind: "user", id: tempId, text, ...(files.length > 0 ? { attachments: localAttachments(files, uploads) } : {}) }]);
    start.mutate(
      { sessionId: sid, input: text, thinking: thinking.current, ...(uploads.length > 0 ? { attachments: uploads } : {}) },
      {
        onSuccess: ({ run_id }) => join(run_id, sid),
        onError: (e) => {
          setItems((prev) => prev.filter((i) => !(i.kind === "user" && i.id === tempId)));
          onRejected?.();
          if (isNotFound(e) && onGone.current) return void onGone.current(sid);
          toast(t("chat.impossibile_avviare_il_run", { error: errorMessage(e) }), "crit");
        },
      },
    );
  }, []);

  const sendAction = useCallback((a: A2uiAction) => {
    const { sessionId: sid, runId: rid, startRun: start, attach: join } = latest.current;
    if (!sid || rid) return;
    setItems((prev) => [...prev, { kind: "action", id: `a-${Date.now()}`, name: a.name, context: a.context }]);
    start.mutate(
      { sessionId: sid, input: actionText(a), kind: "a2ui.action", thinking: thinking.current },
      {
        onSuccess: ({ run_id }) => join(run_id, sid),
        onError: (e) => {
          if (isNotFound(e) && onGone.current) return void onGone.current(sid);
          toast(t("chat.impossibile_inviare_l_azione", { error: errorMessage(e) }), "crit");
        },
      },
    );
  }, []);

  const stop = useCallback(() => {
    const { runId: rid, cancelRun: cancel } = latest.current;
    if (rid) cancel.mutate(rid);
  }, []);

  const respond = useCallback((req: ApprovalRequest, decision: "allow" | "deny", remember = false) => {
    latest.current.respondApproval.mutate(
      { request: req, decision, remember },
      { onSuccess: () => setItems((prev) => prev.map((i) => (i.kind === "approval" && i.req.id === req.id ? { ...i, resolved: decision === "allow" } : i))) },
    );
  }, []);

  const pending = useMemo(() => items.filter((i): i is ApprovalItem => i.kind === "approval" && i.resolved === undefined), [items]);
  const respondingId = respondApproval.isPending ? respondApproval.variables?.request.id ?? null : null;

  return {
    items, runId, running: runId !== null, runState, history, pending, respondingId,
    reload: () => setReloadTick((n) => n + 1),
    send, sendAction, stop, respond,
  };
}
