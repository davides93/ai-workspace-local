// What contains a shell command on each operating system, read from crates/aiw-tools/src/shell.rs.
// Kept as data so the Sandbox tab and its tests say the same thing as the backend does.

import type { Platform } from "../../../shared/api/types";

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
  /** Condition or caveat. */
  note: string;
}

export const CONTAINMENT: Containment[] = [
  {
    os: "linux", label: "Linux", mechanism: "bubblewrap (bwrap)", shell: true,
    writes: "Radice in sola lettura, /tmp privato; la cartella di lavoro è scrivibile solo in «Scrittura nel workspace».",
    network: "Aperta di default. Si chiude (--unshare-net) solo se la chiamata del tool la esclude.",
    note: "Richiede bubblewrap installato: senza, i comandi girano senza contenimento e la modalità Auto chiede conferma per ognuno.",
  },
  {
    os: "macos", label: "macOS", mechanism: "sandbox-exec (profilo Seatbelt)", shell: true,
    writes: "Scritture negate ovunque tranne la cartella di lavoro (in «Scrittura nel workspace») e le cartelle temporanee.",
    network: "Aperta di default. Si chiude ((deny network*)) solo se la chiamata del tool la esclude.",
    note: "Usa lo strumento di sistema /usr/bin/sandbox-exec.",
  },
  {
    os: "windows", label: "Windows", mechanism: "token a integrità bassa e job object", shell: true,
    writes: "Scrive solo nella cartella di lavoro (in «Scrittura nel workspace»).",
    network: "Non isolata: il contenimento di Windows non chiude la rete.",
    note: "I comandi girano in PowerShell; il contenimento è quello del crate aiw-winsandbox.",
  },
  {
    os: "android", label: "Android", mechanism: null, shell: true,
    writes: "Nessun contenimento dei comandi.",
    network: "Aperta.",
    note: "Il backend non rileva una sandbox: i comandi girano con i privilegi dell'app e la modalità Auto chiede conferma per ognuno.",
  },
  {
    os: "ios", label: "iOS", mechanism: null, shell: false,
    writes: "—",
    network: "—",
    note: "iOS non consente di eseguire processi: il tool di shell non è disponibile.",
  },
];

export const containmentFor = (os: string | undefined): Containment | undefined => CONTAINMENT.find((c) => c.os === os);

/** Limits of the current implementation, stated as the code behaves today. */
export const KNOWN_LIMITS: { title: string; body: string }[] = [
  {
    title: "La rete dei comandi è aperta di default",
    body: "Il tool di shell ha un parametro «network» che vale «vero» se non indicato: la sandbox chiude la rete (Linux e macOS) solo quando la chiamata lo chiede. Non esiste un'impostazione che la chiuda per tutti i comandi e su Windows la rete non è isolata. I domini consentiti valgono per web e browser, non per i comandi.",
  },
  {
    title: "Senza sandbox i comandi hanno i tuoi privilegi",
    body: "Succede con la sandbox «Nessun contenimento», su Linux senza bubblewrap e su Android. In quei casi la sola barriera è la conferma: tieni la modalità su «Chiedi» o «Modifica».",
  },
  {
    title: "La sandbox riguarda solo i comandi di shell",
    body: "I tool di file sono limitati alla cartella di lavoro da un controllo sui percorsi (anche attraverso i collegamenti simbolici). Il browser controllato, i server MCP (stdio) e gli hook non passano da questa sandbox: sono processi avviati con i tuoi privilegi.",
  },
  {
    title: "«Senza freni» più «Nessun contenimento» non lascia protezioni",
    body: "Un agente con entrambe le impostazioni può fare ciò che puoi fare tu, senza chiedere. Solo le regole «Nega» della policy e gli hook di blocco restano in vigore.",
  },
];
