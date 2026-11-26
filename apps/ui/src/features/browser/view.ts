import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import type { BrowserViewRequests, BrowserViewStatus } from "../../shared/api/types";
import { errorMessage, errorStatus } from "../../shared/lib/errors";
import { toast } from "../../shared/lib/toast";
import { qk } from "../../shared/query/keys";
import { toFrame, type FrameView } from "./api";

import { t } from "../../shared/i18n";
/** The seconds a yes (or a request) has left, said in minutes for the person ("8 min", "meno di 1 min"). */
export function minutesLeft(seconds: number): string {
  const m = Math.floor(Math.max(0, seconds) / 60);
  return m >= 1 ? t("browser.view_minutes", { n: m }) : t("browser.view_less_than_minute");
}

/** The phone: where its request stands, asked every 2 s while the sheet is open. */
export function useViewStatus(enabled: boolean) {
  const api = useApi();
  return useQuery({ queryKey: [...qk.browserView(), "status"], queryFn: () => api.browserViewStatus(), enabled, refetchInterval: enabled ? 2_000 : false, retry: false });
}

export function useViewRequest() {
  const api = useApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.browserViewRequest(),
    meta: { handled: true },
    onSuccess: (s: BrowserViewStatus) => qc.setQueryData([...qk.browserView(), "status"], s),
    onError: (e) => toast(t("browser.view_request_failed", { error: errorMessage(e) }), "crit"),
  });
}

/**
 * The frames of the page for a phone that has the computer's yes, one after the other, every second. A frame is state of the
 * component that shows it: nothing is cached or kept. A 403 (the yes ended) stops the loop and says so through `onEnded`.
 */
export function useViewFrames(active: boolean, onEnded: () => void): FrameView | null {
  const api = useApi();
  const [frame, setFrame] = useState<FrameView | null>(null);
  useEffect(() => {
    if (!active) {
      setFrame(null);
      return;
    }
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const run = async () => {
      let next = 1_000;
      try {
        const f = await api.browserViewFrame();
        if (stopped) return;
        setFrame(f.running ? toFrame(f) : null);
      } catch (e) {
        if (stopped) return;
        if (errorStatus(e) === 403) {
          onEnded();
          return;
        }
        next = 3_000;
      }
      if (!stopped) timer = setTimeout(() => void run(), next);
    };
    void run();
    return () => {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
      setFrame(null);
    };
  }, [active, api, onEnded]);
  return frame;
}

/**
 * The computer: who asks to look, and who looks now. `null` when this screen may not know (a phone: 403; an older engine: 404);
 * a screen that talks to another computer does not ask at all.
 */
export function useViewRequests(enabled: boolean) {
  const api = useApi();
  return useQuery({
    queryKey: [...qk.browserView(), "requests"],
    queryFn: async (): Promise<BrowserViewRequests | null> => {
      try {
        return await api.browserViewRequests();
      } catch (e) {
        const s = errorStatus(e);
        if (s === 403 || s === 404) return null;
        throw e;
      }
    },
    enabled,
    refetchInterval: (q) => (q.state.data === null ? false : 4_000),
    retry: false,
  });
}

export function useViewDecision() {
  const api = useApi();
  const qc = useQueryClient();
  const refresh = () => void qc.invalidateQueries({ queryKey: qk.browserView() });
  return {
    decide: useMutation({
      mutationFn: (v: { id: string; approve: boolean }) => api.browserViewDecide(v.id, v.approve),
      meta: { handled: true },
      onError: (e) => toast(errorMessage(e), "crit"),
      onSettled: refresh,
    }),
    revoke: useMutation({
      mutationFn: () => api.browserViewRevoke(),
      meta: { handled: true },
      onError: (e) => toast(errorMessage(e), "crit"),
      onSettled: refresh,
    }),
  };
}
