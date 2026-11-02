import type { PolicyMode, SandboxMode, ToolRisk } from "../api/types";
import type { PillTone } from "../ui/Pill";

/** Approval modes of an agent (crates/aiw-agent/src/policy.rs), with what each one really does. */
export const APPROVAL_MODES: { id: PolicyMode; label: string; summary: string; detail: string; tone: PillTone }[] = [
  {
    id: "plan", label: "Piano", tone: "info",
    summary: "Solo lettura: analizza e propone, non modifica nulla.",
    detail: "Consente lettura e rete; nega scritture, comandi e azioni su mouse e tastiera.",
  },
  {
    id: "ask", label: "Chiedi", tone: "ok",
    summary: "Chiede conferma per ogni modifica, comando o azione sul computer.",
    detail: "Legge e naviga da solo; prima di scrivere file, eseguire comandi o usare mouse e tastiera chiede il tuo consenso.",
  },
  {
    id: "auto-edit", label: "Modifica", tone: "warn",
    summary: "Modifica i file da solo, chiede per i comandi.",
    detail: "Lettura, rete e scrittura di file senza conferma; i comandi e le azioni su mouse e tastiera chiedono il consenso.",
  },
  {
    id: "auto", label: "Auto", tone: "warn",
    summary: "Esegue anche i comandi, ma dentro la sandbox.",
    detail: "Chiede solo per mouse e tastiera e, se il sistema non ha una sandbox, per ogni comando.",
  },
  {
    id: "yolo", label: "Senza freni", tone: "crit",
    summary: "Nessuna conferma: sconsigliato.",
    detail: "Consente tutto, comprese le azioni distruttive. Le regole di rifiuto della policy restano valide.",
  },
];

export const approvalMode = (id: string) => APPROVAL_MODES.find((m) => m.id === id);

/** Sandbox modes of an agent: how far its commands and file tools can reach. */
export const SANDBOX_MODES: { id: SandboxMode; label: string; summary: string; tone: PillTone }[] = [
  { id: "read-only", label: "Sola lettura", tone: "ok", summary: "I comandi leggono ma non scrivono; i tool di file non modificano nulla." },
  { id: "workspace-write", label: "Scrittura nel workspace", tone: "ok", summary: "Comandi e tool di file scrivono solo dentro la cartella di lavoro." },
  { id: "off", label: "Nessun contenimento", tone: "crit", summary: "Comandi e tool di file agiscono con i tuoi privilegi, anche fuori dal workspace." },
];

export const sandboxMode = (id: string) => SANDBOX_MODES.find((m) => m.id === id);

/** Same labels and tones as the tool table of "Tool, MCP e browser". */
export const RISK_META: Record<ToolRisk, { label: string; tone: PillTone; hint: string }> = {
  read: { label: "lettura", tone: "ok", hint: "Legge dati, non cambia nulla." },
  write: { label: "scrittura", tone: "warn", hint: "Crea o modifica file." },
  exec: { label: "esecuzione", tone: "crit", hint: "Esegue comandi di shell." },
  network: { label: "rete", tone: "info", hint: "Accede a pagine web o servizi di rete." },
  computer: { label: "computer-use", tone: "crit", hint: "Controlla mouse, tastiera o il desktop." },
};
