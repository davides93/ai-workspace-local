import type { PolicyMode, SandboxMode, ToolRisk } from "../api/types";
import type { PillTone } from "../ui";

import { t } from "../i18n";
/** Approval modes of an agent (crates/aiw-agent/src/policy.rs), with what each one really does. */
export const APPROVAL_MODES: { id: PolicyMode; label: string; summary: string; detail: string; tone: PillTone }[] = [
  {
    id: "plan", get label() { return t("lib.piano"); }, tone: "info",
    get summary() { return t("lib.solo_lettura_analizza_e_propone"); },
    get detail() { return t("lib.consente_lettura_e_rete_nega"); },
  },
  {
    id: "ask", get label() { return t("lib.chiedi"); }, tone: "ok",
    get summary() { return t("lib.chiede_conferma_per_ogni_modifica"); },
    get detail() { return t("lib.legge_e_naviga_da_solo"); },
  },
  {
    id: "auto-edit", get label() { return t("lib.modifica"); }, tone: "warn",
    get summary() { return t("lib.modifica_i_file_da_solo"); },
    get detail() { return t("lib.lettura_rete_e_scrittura_di"); },
  },
  {
    id: "auto", get label() { return t("lib.auto"); }, tone: "warn",
    get summary() { return t("lib.esegue_anche_i_comandi_ma"); },
    get detail() { return t("lib.chiede_solo_per_mouse_e"); },
  },
  {
    id: "yolo", get label() { return t("lib.senza_freni"); }, tone: "crit",
    get summary() { return t("lib.nessuna_conferma_sconsigliato"); },
    get detail() { return t("lib.consente_tutto_comprese_le_azioni"); },
  },
];

export const approvalMode = (id: string) => APPROVAL_MODES.find((m) => m.id === id);

/** Sandbox modes of an agent: how far its commands and file tools can reach. */
export const SANDBOX_MODES: { id: SandboxMode; label: string; summary: string; tone: PillTone }[] = [
  { id: "read-only", get label() { return t("lib.sola_lettura"); }, tone: "ok", get summary() { return t("lib.i_comandi_leggono_ma_non"); } },
  { id: "workspace-write", get label() { return t("lib.scrittura_nel_workspace"); }, tone: "ok", get summary() { return t("lib.comandi_e_tool_di_file"); } },
  { id: "off", get label() { return t("lib.nessun_contenimento"); }, tone: "crit", get summary() { return t("lib.comandi_e_tool_di_file_2"); } },
];

export const sandboxMode = (id: string) => SANDBOX_MODES.find((m) => m.id === id);

/** Same labels and tones as the tool table of "Tool, MCP e browser". */
export const RISK_META: Record<ToolRisk, { label: string; tone: PillTone; hint: string }> = {
  read: { get label() { return t("lib.lettura"); }, tone: "ok", get hint() { return t("lib.legge_dati_non_cambia_nulla"); } },
  write: { get label() { return t("lib.scrittura"); }, tone: "warn", get hint() { return t("lib.crea_o_modifica_file"); } },
  exec: { get label() { return t("lib.esecuzione"); }, tone: "crit", get hint() { return t("lib.esegue_comandi_di_shell"); } },
  network: { get label() { return t("lib.rete"); }, tone: "info", get hint() { return t("lib.accede_a_pagine_web_o"); } },
  computer: { get label() { return t("lib.computer_use"); }, tone: "crit", get hint() { return t("lib.controlla_mouse_tastiera_o_il"); } },
};
