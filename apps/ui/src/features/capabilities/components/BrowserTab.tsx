import type { ToolInfo } from "../../../shared/api/types";
import { Link } from "../../../app/router";
import { DataTable, EmptyState, Icon, Panel, Pill, QueryState, Section, type Column, type PillTone } from "../../../shared/ui";
import { useSetToolsEnabled, useTools } from "../api";
import { capabilityStatus, isBrowserTool, isComputerTool, type CapabilityStatus } from "../model/tools";
import { ToolRow } from "./ToolRow";

const STATUS: Record<CapabilityStatus, { label: string; tone: PillTone }> = {
  ready: { label: "Disponibile", tone: "ok" },
  partial: { label: "Parziale", tone: "warn" },
  off: { label: "Disattivato", tone: "neutral" },
  unavailable: { label: "Non disponibile qui", tone: "crit" },
  absent: { label: "Non presente in questa build", tone: "neutral" },
};

/** Approval of each risk class per approval mode, as `aiw-agent::policy` decides it (before the policy rules). */
const MODES = ["plan", "ask", "auto-edit", "auto", "yolo"] as const;
type Cell = { label: string; tone: PillTone };
const ALLOW: Cell = { label: "consentito", tone: "ok" };
const ASK: Cell = { label: "chiede", tone: "warn" };
const DENY: Cell = { label: "negato", tone: "crit" };
const MATRIX: { risk: string; cells: Cell[] }[] = [
  { risk: "Lettura e rete (snapshot, screenshot, navigazione)", cells: [ALLOW, ALLOW, ALLOW, ALLOW, ALLOW] },
  { risk: "Computer-use (clic, digitazione, tasti, scroll)", cells: [DENY, ASK, ASK, ASK, ALLOW] },
];

interface Capability {
  id: "browser" | "computer";
  title: string;
  icon: string;
  tools: ToolInfo[];
  how: string[];
}

function ApprovalMatrix() {
  const columns: Column<(typeof MATRIX)[number]>[] = [
    { key: "risk", header: "Azione", render: (r) => r.risk },
    ...MODES.map((m, i) => ({ key: m, header: <span className="mono">{m}</span>, nowrap: true, render: (r: (typeof MATRIX)[number]) => <Pill tone={r.cells[i].tone}>{r.cells[i].label}</Pill> })),
  ];
  return <DataTable label="Approvazione per modalità" columns={columns} rows={MATRIX} rowKey={(r) => r.risk} />;
}

/** What this build can do with the browser and the desktop, how it does it, and when it asks. */
export function BrowserTab() {
  const tools = useTools();
  const setEnabled = useSetToolsEnabled();
  return (
    <QueryState query={tools}>
      {(all) => {
        const caps: Capability[] = [
          {
            id: "browser", title: "Browser controllato", icon: "globe", tools: all.filter(isBrowserTool),
            how: [
              "Avvia un profilo dedicato di Chrome, Edge o Chromium già installato sul computer e lo guida con il Chrome DevTools Protocol (CDP).",
              "Lo snapshot è testo con gli elementi numerati: funziona anche con modelli senza visione. Lo screenshot serve ai modelli con visione.",
              "I siti raggiungibili si limitano con «Domini consentiti» nelle Impostazioni; vuoto significa tutti i siti pubblici.",
            ],
          },
          {
            id: "computer", title: "Computer-use", icon: "mouse", tools: all.filter(isComputerTool),
            how: [
              "Cattura lo schermo principale e invia clic, testo e tasti sintetici alla finestra attiva.",
              "Le coordinate si riferiscono all'ultimo screenshot, scalato a 1280 px di larghezza al massimo.",
              "Disponibile solo nelle build desktop con la feature computer-use; le altre elencano i tool come non disponibili.",
            ],
          },
        ];
        return (
          <>
            <div className="cap-grid">
              {caps.map((c) => {
                const st = capabilityStatus(c.tools);
                return (
                  <Panel key={c.id} icon={c.icon} title={<>{c.title} <Pill tone={STATUS[st.status].tone}>{STATUS[st.status].label}</Pill></>} padded={false}>
                    <div className="cap-body">
                      {st.reason && <p className="cap-note cap-note-warn text-sm"><Icon name="alert" size={15} /> {st.reason}</p>}
                      <ul className="cap-how text-sm">{c.how.map((h) => <li key={h}>{h}</li>)}</ul>
                    </div>
                    {c.tools.length === 0 ? (
                      <p className="cap-empty faint text-sm">Nessun tool <code>{c.id}_*</code> registrato da questo backend.</p>
                    ) : (
                      <ul className="cap-tools">
                        {c.tools.map((t) => <ToolRow key={t.name} tool={t} onToggle={(enabled) => setEnabled.mutate({ names: [t.name], enabled })} />)}
                      </ul>
                    )}
                  </Panel>
                );
              })}
            </div>

            <Section title="Quando chiedono conferma" hint="Dipende dalla modalità di approvazione dell'agente; le regole della policy hanno la precedenza.">
              <ApprovalMatrix />
              <p className="muted text-sm">
                «Consenti sempre per la sessione» salta la richiesta fino alla fine della sessione. Il kill switch chiude il browser controllato e ferma i run.
                Modalità e regole si cambiano in <Link to="/agents">Agenti</Link> e <Link to="/security">Sicurezza</Link>.
              </p>
            </Section>

            <Section title="Ultime catture" hint="Gli screenshot prodotti da browser e computer-use.">
              <EmptyState
                icon="mouse" title="Le catture appaiono nella chat"
                description="Mentre un run usa browser_screenshot o computer_screenshot, l'immagine compare nella scheda del tool, sotto la riga del messaggio. Questa pagina non conserva uno storico delle catture."
                action={<Link to="/chat" className="btn"><Icon name="chat" size={14} /> Apri il Workspace</Link>}
              />
            </Section>
          </>
        );
      }}
    </QueryState>
  );
}

