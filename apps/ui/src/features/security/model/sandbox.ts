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
  /** Can the app folder and the credential folders be hidden from a command's reads? */
  reads: string;
  /** Can an MCP server (stdio) be isolated the same way? */
  mcp: string;
  /** Condition or caveat. */
  note: string;
}

export const CONTAINMENT: Containment[] = [
  {
    os: "linux", label: "Linux", mechanism: "bubblewrap (bwrap)", shell: true,
    writes: "Radice in sola lettura, /tmp privato; la cartella di lavoro è scrivibile solo in «Scrittura nel workspace».",
    network: "Chiusa di default (--unshare-net). Si apre con l'impostazione «Rete dei comandi».",
    reads: "Sì: la cartella dei dati dell'app e quelle delle credenziali (.ssh, .aws, …) sembrano vuote ai comandi; /proc del processo principale non è visibile.",
    mcp: "Sì, con lo stesso meccanismo.",
    note: "Richiede bubblewrap installato: senza, i comandi girano senza contenimento e la modalità Auto chiede conferma per ognuno.",
  },
  {
    os: "macos", label: "macOS", mechanism: "sandbox-exec (profilo Seatbelt)", shell: true,
    writes: "Scritture negate ovunque tranne la cartella di lavoro (in «Scrittura nel workspace») e le cartelle temporanee.",
    network: "Chiusa di default ((deny network*)). Si apre con l'impostazione «Rete dei comandi».",
    reads: "Sì, con regole del profilo Seatbelt. Non è stato verificato su un Mac reale in questa versione.",
    mcp: "Sì, con lo stesso meccanismo (non verificato su un Mac reale).",
    note: "Usa lo strumento di sistema /usr/bin/sandbox-exec.",
  },
  {
    os: "windows", label: "Windows", mechanism: "token a integrità bassa e job object", shell: true,
    writes: "Scrive solo nella cartella di lavoro (in «Scrittura nel workspace»).",
    network: "Non isolata: su Windows la rete dei comandi non si può chiudere, qualunque sia l'impostazione.",
    reads: "No: il token a integrità bassa limita le scritture, non le letture. Le cartelle nascoste valgono solo per i tool di file degli agenti, non per i comandi di shell.",
    mcp: "No: i server MCP non si possono isolare su Windows; restano «non isolati».",
    note: "I comandi girano in PowerShell; il contenimento è quello del crate aiw-winsandbox.",
  },
  {
    os: "android", label: "Android", mechanism: null, shell: true,
    writes: "Nessun contenimento dei comandi.",
    network: "Aperta.",
    reads: "No.",
    mcp: "No.",
    note: "Il backend non rileva una sandbox: i comandi girano con i privilegi dell'app e la modalità Auto chiede conferma per ognuno.",
  },
  {
    os: "ios", label: "iOS", mechanism: null, shell: false,
    writes: "—",
    network: "—",
    reads: "—",
    mcp: "—",
    note: "iOS non consente di eseguire processi: il tool di shell non è disponibile.",
  },
];

export const containmentFor = (os: string | undefined): Containment | undefined => CONTAINMENT.find((c) => c.os === os);

/** Limits of the current implementation, stated as the code behaves today. */
export const KNOWN_LIMITS: { title: string; body: string }[] = [
  {
    title: "Su Windows la rete dei comandi non si può chiudere",
    body: "Su Linux e macOS la rete dei comandi è chiusa di default e si apre da qui. Su Windows il contenimento (token a integrità bassa) non isola la rete né nasconde cartelle in lettura: l'interruttore non ha effetto sui comandi. Servirebbe un AppContainer o regole del firewall, che non sono implementati. I domini consentiti valgono per web e browser, non per i comandi.",
  },
  {
    title: "Senza sandbox i comandi hanno i tuoi privilegi",
    body: "Succede con la sandbox «Nessun contenimento», su Linux senza bubblewrap e su Android. In quei casi la sola barriera è la conferma: tieni la modalità su «Chiedi» o «Modifica». Le variabili d'ambiente con chiavi e token sono tolte ai comandi in ogni caso.",
  },
  {
    title: "Le regole della policy leggono il testo del comando",
    body: "Un comando composto (con ;, &&, |, $(…)) è letto parte per parte: «Consenti» vale solo se ogni parte è consentita, «Nega» se una qualsiasi lo è. Ma wrapper che nascondono il comando vero (ssh host «…», docker run …, interpreti con script propri) non si aprono: una regola «Consenti» non li copre e si ricade su «Chiedi». La policy non sostituisce la sandbox.",
  },
  {
    title: "Browser controllato, hook e valutatori non passano da questa sandbox",
    body: "I tool di file sono limitati alla cartella di lavoro da un controllo sui percorsi (anche attraverso i collegamenti simbolici) e rispettano le cartelle nascoste. Il browser controllato, gli hook e i valutatori degli eval sono processi avviati con i tuoi privilegi. I server MCP stdio possono essere isolati (Linux, macOS) ma quelli già presenti restano «non isolati» finché non premi «Isola».",
  },
  {
    title: "«Senza freni» più «Nessun contenimento» non lascia protezioni",
    body: "Un agente con entrambe le impostazioni può fare ciò che puoi fare tu, senza chiedere. Solo le regole «Nega» della policy e gli hook di blocco restano in vigore.",
  },
];
