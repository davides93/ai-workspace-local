// What contains a shell command on each operating system, read from crates/aiw-tools/src/shell.rs.
// Kept as data so the Sandbox tab and its tests say the same thing as the backend does.

import type { Platform } from "../../../shared/api/types";

import { t } from "../../../shared/i18n";
export interface Containment {
  os: Platform;
  label: string;
  /** Mechanism the backend uses, or null when shell commands cannot be contained. */
  mechanism: string | null;
  /** Does the shell tool work at all? */
  shell: boolean;
  /** Writes outside the workspace are refused for sandboxed commands. */
  writes: string;
  /** Can the network of a command be closed? */
  network: string;
  /** Can the app folder and the credential folders be hidden from a command's reads? */
  reads: string;
  /** Can an MCP server (stdio) be isolated the same way? */
  mcp: string;
  /** Condition or caveat. */
  note: string;
}

export const CONTAINMENT: Containment[] = [
  {
    os: "linux", get label() { return t("security.linux"); }, get mechanism() { return t("security.bubblewrap_bwrap"); }, shell: true,
    get writes() { return t("security.radice_in_sola_lettura_tmp"); },
    get network() { return t("security.chiusa_di_default_unshare_net"); },
    get reads() { return t("security.si_la_cartella_dei_dati"); },
    get mcp() { return t("security.si_con_lo_stesso_meccanismo"); },
    get note() { return t("security.richiede_bubblewrap_installato_senza_i"); },
  },
  {
    os: "macos", get label() { return t("security.macos"); }, get mechanism() { return t("security.sandbox_exec_profilo_seatbelt"); }, shell: true,
    get writes() { return t("security.scritture_negate_ovunque_tranne_la"); },
    get network() { return t("security.chiusa_di_default_deny_network"); },
    get reads() { return t("security.si_con_regole_del_profilo"); },
    get mcp() { return t("security.si_con_lo_stesso_meccanismo_2"); },
    get note() { return t("security.sandbox_exec_note"); },
  },
  {
    os: "windows", get label() { return t("security.windows"); }, get mechanism() { return t("security.token_a_integrita_bassa_e"); }, shell: true,
    get writes() { return t("security.scrive_solo_nella_cartella_di"); },
    get network() { return t("security.non_isolata_su_windows_la"); },
    get reads() { return t("security.no_il_token_a_integrita"); },
    get mcp() { return t("security.no_i_server_mcp_non"); },
    get note() { return t("security.i_comandi_girano_in_powershell"); },
  },
  {
    os: "android", get label() { return t("security.android"); }, mechanism: null, shell: true,
    get writes() { return t("security.nessun_contenimento_dei_comandi"); },
    network: "Aperta.",
    reads: "No.",
    mcp: "No.",
    get note() { return t("security.il_backend_non_rileva_una"); },
  },
  {
    os: "ios", get label() { return t("security.ios"); }, mechanism: null, shell: false,
    writes: "—",
    network: "—",
    reads: "—",
    mcp: "—",
    get note() { return t("security.ios_non_consente_di_eseguire"); },
  },
];

export const containmentFor = (os: string | undefined): Containment | undefined => CONTAINMENT.find((c) => c.os === os);

/** Limits of the current implementation, stated as the code behaves today. */
export const KNOWN_LIMITS: { title: string; body: string }[] = [
  {
    get title() { return t("security.su_windows_la_rete_dei_2"); },
    get body() { return t("security.su_linux_e_macos_la"); },
  },
  {
    get title() { return t("security.senza_sandbox_i_comandi_hanno"); },
    get body() { return t("security.succede_con_la_sandbox_nessun"); },
  },
  {
    get title() { return t("security.le_regole_della_policy_leggono"); },
    get body() { return t("security.un_comando_composto_con_e"); },
  },
  {
    get title() { return t("security.browser_controllato_hook_e_valutatori"); },
    get body() { return t("security.i_tool_di_file_sono"); },
  },
  {
    get title() { return t("security.senza_freni_piu_nessun_contenimento"); },
    get body() { return t("security.un_agente_con_entrambe_le"); },
  },
];
