import { Led, Pill, type LedTone, type PillTone } from "./Pill";

/** Maps any status string used by the API (runtime, MCP, run, download, peer) to a tone. */
export function statusTone(s: string): LedTone {
  switch (s) {
    case "running":
    case "connected":
    case "succeeded":
    case "completed":
    case "done":
    case "applied":
    case "online":
      return "ok";
    case "starting":
    case "queued":
    case "verifying":
    case "waiting_approval":
    case "waiting":
      return "run";
    case "error":
    case "failed":
      return "crit";
    default:
      return "off";
  }
}

export const statusLabel: Record<string, string> = {
  running: "In esecuzione", starting: "Avvio…", available: "Disponibile", installable: "Da installare", unsupported: "Non supportato",
  stopped: "Fermo", error: "Errore", connected: "Connesso", disconnected: "Disconnesso", succeeded: "Completato", completed: "Completato", failed: "Fallito",
  waiting_approval: "Attende conferma", cancelled: "Annullato", pending: "In coda", done: "Fatto", applied: "Applicato", skipped: "Saltato", waiting: "In attesa",
  online: "Online", offline: "Offline", queued: "In coda", verifying: "Verifica",
};

export function StatusPill({ status }: { status: string }) {
  const tone = statusTone(status);
  const pillTone: PillTone = tone === "off" ? "neutral" : tone;
  return (
    <Pill tone={pillTone}>
      <Led tone={tone} small />
      {statusLabel[status] ?? status}
    </Pill>
  );
}
