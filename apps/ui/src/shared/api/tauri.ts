import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ApiError, HttpTransport } from "./http";
import type { TransportCapabilities } from "./transport";
import { blobToBase64 } from "../lib/base64";
import type { AgEvent, BackgroundStatus, Transcript } from "./types";

interface IpcResponse {
  status: number;
  body: unknown;
}

/**
 * In-app IPC carrier. The Rust shell dispatches `aiw_request` through the very same
 * axum router used by `aiw serve` (tower `oneshot`, no socket), so routes and payloads
 * are identical to HTTP. Run events arrive as Tauri events instead of SSE.
 */
export class TauriTransport extends HttpTransport {
  override readonly kind = "tauri" as const;
  override readonly capabilities: TransportCapabilities = { demo: false, native: true, pairing: false };

  constructor() {
    super("", null);
  }

  protected override async req<T>(method: string, path: string, body?: unknown): Promise<T> {
    const res = await invoke<IpcResponse>("aiw_request", { method, path: `/api${path}`, body: body ?? null });
    if (res.status >= 400) {
      const msg = (res.body as { error?: string } | null)?.error ?? `HTTP ${res.status}`;
      throw new ApiError(res.status, msg);
    }
    return res.body as T;
  }

  /** The native side of the background checks (`aiw_background` in the shell; the desktop answers "not supported"). */
  private background = (action: "status" | "request_notifications" | "configure" | "check_now") => invoke<BackgroundStatus>("aiw_background", { action });
  backgroundStatus = () => this.background("status");
  backgroundRequestNotifications = () => this.background("request_notifications");
  backgroundConfigure = () => this.background("configure");
  backgroundCheckNow = () => this.background("check_now");

  async openBrowser(): Promise<boolean> {
    return invoke<boolean>("aiw_browser_show");
  }

  /** The IPC bridge carries JSON only: the audio travels as base64. */
  override async transcribe(wav: Blob, language?: string): Promise<Transcript> {
    return this.req<Transcript>("POST", "/voice/transcribe", {
      audio: await blobToBase64(wav),
      mime: wav.type || "audio/wav",
      language: language && language !== "auto" ? language : undefined,
    });
  }

  override subscribeRun(runId: string, onEvent: (e: AgEvent) => void): () => void {
    let unlisten: (() => void) | null = null;
    let closed = false;
    let lastSeq = -1;
    const deliver = (seq: number, ev: AgEvent) => {
      if (seq <= lastSeq) return; // replay and live stream can overlap
      lastSeq = seq;
      onEvent(ev);
    };
    const buffered: { seq: number; event: AgEvent }[] = [];
    let replayed = false;
    listen<{ run_id: string; seq: number; event: AgEvent }>("aiw://run-event", (msg) => {
      if (msg.payload.run_id !== runId) return;
      if (!replayed) buffered.push(msg.payload);
      else deliver(msg.payload.seq, msg.payload.event);
    }).then((fn) => {
      if (closed) fn();
      else unlisten = fn;
    });
    // Replay what happened before the listener was attached, then flush the live buffer.
    invoke<{ seq: number; event: AgEvent }[]>("aiw_run_events", { runId })
      .catch(() => [] as { seq: number; event: AgEvent }[])
      .then((evs) => {
        evs.forEach((e) => deliver(e.seq, e.event));
        replayed = true;
        buffered.splice(0).forEach((e) => deliver(e.seq, e.event));
      });
    return () => {
      closed = true;
      unlisten?.();
    };
  }
}
