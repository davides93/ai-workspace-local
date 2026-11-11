import type { ProactiveStatus, Proposal, ProposalKind, ProposalStatus, TickResult } from "../types";

// Demo data and rules of the proactive API, as the engine applies them (crates/aiw-engine/src/proactive.rs).

const iso = (minAgo: number) => new Date(Date.now() - minAgo * 60_000).toISOString();
const week = () => new Date(Date.now() + 7 * 86_400_000).toISOString();

export const PROPOSAL_KINDS: readonly ProposalKind[] = ["proposal", "briefing"];
export const PROPOSAL_STATUSES: readonly ProposalStatus[] = ["open", "approved", "dismissed", "expired", "done"];

export const proposals: Proposal[] = [
  {
    id: "prp_demo_briefing", kind: "briefing", title: "Il riepilogo di oggi",
    noticed: "Buongiorno. Oggi hai un promemoria: rinnovare l'abbonamento del treno (tra 5 giorni). Hai 2 proposte in attesa e ieri hai finito 3 lavori.",
    instruction: "", source: "briefing:demo", agent_id: "operator", status: "open", created_at: iso(40), expires_at: week(),
  },
  {
    id: "prp_demo_files", kind: "proposal", title: "Riordinare le fatture in Scaricati",
    noticed: "Sono arrivati 4 file nuovi in Scaricati: sembrano fatture dei fornitori.",
    instruction: "Guarda i file nuovi in Scaricati e proponi dove spostare le fatture, senza spostare nulla finché non ti dico di sì.",
    source: "files:~/Scaricati:1", agent_id: "operator", status: "open", created_at: iso(55), expires_at: week(),
  },
  {
    id: "prp_demo_reminder", kind: "proposal", title: "Promemoria: rinnovare l'abbonamento del treno",
    noticed: "Avevi chiesto di ricordartelo: «Rinnovare l'abbonamento del treno».",
    instruction: "Aiutami con questo promemoria: rinnovare l'abbonamento del treno.",
    source: "reminder:mem_demo_reminder", agent_id: "operator", status: "open", created_at: iso(70), expires_at: week(),
  },
  {
    id: "prp_demo_done", kind: "proposal", title: "Riassumere le note della riunione",
    noticed: "C'era un file di note nuovo nella cartella Documenti.",
    instruction: "Riassumi le note della riunione in cinque punti.",
    source: "files:~/Documenti:1", agent_id: "operator", status: "done", created_at: iso(1500), expires_at: week(), decided_at: iso(1400), run_id: "run_demo_done", session_id: "ses_pdf",
  },
];

export const proactiveState = {
  level: "propose" as ProactiveStatus["level"],
  lastTick: undefined as TickResult | undefined,
};
