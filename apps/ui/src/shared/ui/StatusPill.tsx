import { Led, Pill, type LedTone, type PillTone } from "./Pill";

import { t } from "../i18n";
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
    case "connecting":
    case "reconnecting":
    case "auth_required":
    case "approval_required":
      return "run";
    case "error":
    case "unreachable":
    case "failed":
      return "crit";
    default:
      return "off";
  }
}

export const statusLabel: Record<string, string> = {
  running: "In esecuzione", starting: "Avvio…", get available() { return t("ui.disponibile"); }, installable: "Da installare", unsupported: "Non supportato",
  get stopped() { return t("ui.fermo"); }, get error() { return t("ui.errore"); }, get connected() { return t("ui.connesso"); }, get disconnected() { return t("ui.disconnesso"); }, get succeeded() { return t("ui.completato"); }, get completed() { return t("ui.completato"); }, get failed() { return t("ui.fallito"); },
  get waiting_approval() { return t("ui.attende_conferma"); }, get cancelled() { return t("ui.annullato"); }, pending: "In coda", get done() { return t("ui.fatto"); }, get applied() { return t("ui.applicato"); }, get skipped() { return t("ui.saltato"); }, waiting: "In attesa",
  reconnecting: "Riconnessione…", connecting: "Connessione…", unreachable: "Non raggiungibile", get auth_required() { return t("ui.serve_l_accesso"); }, approval_required: "Da approvare",
  get online() { return t("ui.online"); }, get offline() { return t("ui.offline"); }, queued: "In coda", get verifying() { return t("ui.verifica"); },
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
