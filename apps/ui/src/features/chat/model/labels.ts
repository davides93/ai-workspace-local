import type { PolicyMode, RunStatus, SandboxMode } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";

import { t } from "../../../shared/i18n";
export interface LabelInfo {
  label: string;
  hint: string;
  tone: PillTone;
}

/** Approval mode of an agent: how much it may do before asking. */
export const MODE_INFO: Record<PolicyMode, LabelInfo> = {
  plan: { get label() { return t("chat.piano"); }, get hint() { return t("chat.solo_lettura_propone_non_modifica"); }, tone: "info" },
  ask: { get label() { return t("chat.chiedi"); }, get hint() { return t("chat.conferma_per_ogni_effetto_collaterale"); }, tone: "info" },
  "auto-edit": { get label() { return t("chat.modifica"); }, get hint() { return t("chat.modifica_i_file_da_solo"); }, tone: "warn" },
  auto: { get label() { return t("chat.auto"); }, get hint() { return t("chat.tutto_ma_sempre_dentro_la"); }, tone: "warn" },
  yolo: { get label() { return t("chat.senza_freni"); }, get hint() { return t("chat.nessuna_conferma_sconsigliato"); }, tone: "crit" },
};

/** What the tools of an agent may touch. */
export const SANDBOX_INFO: Record<SandboxMode, LabelInfo> = {
  "read-only": { get label() { return t("chat.sola_lettura"); }, get hint() { return t("chat.gli_strumenti_non_possono_scrivere"); }, tone: "neutral" },
  "workspace-write": { get label() { return t("chat.scrive_nel_workspace"); }, get hint() { return t("chat.puo_scrivere_solo_dentro_la"); }, tone: "neutral" },
  off: { get label() { return t("chat.sandbox_spenta"); }, get hint() { return t("chat.nessun_confine_gli_strumenti_vedono"); }, tone: "crit" },
};

const UNKNOWN_MODE: LabelInfo = { label: "—", hint: "", tone: "neutral" };

export const modeInfo = (m: string | undefined): LabelInfo => (m ? MODE_INFO[m as PolicyMode] ?? { ...UNKNOWN_MODE, label: m } : UNKNOWN_MODE);
export const sandboxInfo = (m: string | undefined): LabelInfo => (m ? SANDBOX_INFO[m as SandboxMode] ?? { ...UNKNOWN_MODE, label: m } : UNKNOWN_MODE);

/** Last run of a session as a word. */
export const RUN_STATUS_LABEL: Record<RunStatus, string> = {
  get running() { return t("chat.in_esecuzione"); },
  get waiting_approval() { return t("chat.in_attesa_di_conferma"); },
  get succeeded() { return t("chat.completato_2"); },
  get failed() { return t("chat.non_riuscito"); },
  get cancelled() { return t("chat.interrotto_2"); },
};
