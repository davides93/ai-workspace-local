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
  get running() { return t("ui.status_running"); }, get starting() { return t("ui.status_starting"); }, get available() { return t("ui.disponibile"); }, get installable() { return t("ui.status_installable"); }, get unsupported() { return t("ui.status_unsupported"); },
  get stopped() { return t("ui.fermo"); }, get error() { return t("ui.errore"); }, get connected() { return t("ui.connesso"); }, get disconnected() { return t("ui.disconnesso"); }, get succeeded() { return t("ui.completato"); }, get completed() { return t("ui.completato"); }, get failed() { return t("ui.fallito"); },
  get waiting_approval() { return t("ui.attende_conferma"); }, get cancelled() { return t("ui.annullato"); }, get pending() { return t("ui.status_pending"); }, get done() { return t("ui.fatto"); }, get applied() { return t("ui.applicato"); }, get skipped() { return t("ui.saltato"); }, get waiting() { return t("ui.status_waiting"); },
  get reconnecting() { return t("ui.status_reconnecting"); }, get connecting() { return t("ui.status_connecting"); }, get unreachable() { return t("ui.status_unreachable"); }, get auth_required() { return t("ui.serve_l_accesso"); }, get approval_required() { return t("ui.status_approval_required"); },
  get online() { return t("ui.online"); }, get offline() { return t("ui.offline"); }, get queued() { return t("ui.status_queued"); }, get verifying() { return t("ui.verifica"); },
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
