import type { PillTone } from "../../../shared/ui/Pill";
import type { PolicyMode, RunStatus, SandboxMode } from "../../../shared/api/types";

export interface LabelInfo {
  label: string;
  hint: string;
  tone: PillTone;
}

/** Approval mode of an agent: how much it may do before asking. */
export const MODE_INFO: Record<PolicyMode, LabelInfo> = {
  plan: { label: "Piano", hint: "Solo lettura: propone, non modifica", tone: "info" },
  ask: { label: "Chiedi", hint: "Conferma per ogni effetto collaterale", tone: "info" },
  "auto-edit": { label: "Modifica", hint: "Modifica i file da solo, chiede per i comandi", tone: "warn" },
  auto: { label: "Auto", hint: "Tutto, ma sempre dentro la sandbox", tone: "warn" },
  yolo: { label: "Senza freni", hint: "Nessuna conferma: sconsigliato", tone: "crit" },
};

/** What the tools of an agent may touch. */
export const SANDBOX_INFO: Record<SandboxMode, LabelInfo> = {
  "read-only": { label: "Sola lettura", hint: "Gli strumenti non possono scrivere file", tone: "neutral" },
  "workspace-write": { label: "Scrive nel workspace", hint: "Può scrivere solo dentro la cartella di lavoro", tone: "neutral" },
  off: { label: "Sandbox spenta", hint: "Nessun confine: gli strumenti vedono tutto il disco", tone: "crit" },
};

const UNKNOWN_MODE: LabelInfo = { label: "—", hint: "", tone: "neutral" };

export const modeInfo = (m: string | undefined): LabelInfo => (m ? MODE_INFO[m as PolicyMode] ?? { ...UNKNOWN_MODE, label: m } : UNKNOWN_MODE);
export const sandboxInfo = (m: string | undefined): LabelInfo => (m ? SANDBOX_INFO[m as SandboxMode] ?? { ...UNKNOWN_MODE, label: m } : UNKNOWN_MODE);

/** Last run of a session as a word. */
export const RUN_STATUS_LABEL: Record<RunStatus, string> = {
  running: "In esecuzione",
  waiting_approval: "In attesa di conferma",
  succeeded: "Completato",
  failed: "Non riuscito",
  cancelled: "Interrotto",
};
