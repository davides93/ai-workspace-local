import { Link, navigate, useParams } from "../../app/router";
import { Page, PageHeader, Panel, Pill, QueryState, Section, TabPanel, Tabs, KeyValue } from "../../shared/ui";
import { useConfig } from "../settings/api";
import { HooksEditor } from "./components/HooksEditor";
import { PolicyEditor } from "./components/PolicyEditor";

type Tab = "policy" | "hooks" | "sandbox";
const TABS: Tab[] = ["policy", "hooks", "sandbox"];

const MODE_HINT: Record<string, string> = {
  plan: "Solo lettura: l'agente propone, non modifica.",
  ask: "Conferma per ogni effetto collaterale.",
  "auto-edit": "Modifica i file da solo, chiede per i comandi.",
  auto: "Tutto, ma sempre dentro la sandbox.",
  yolo: "Nessuna conferma: sconsigliato.",
};
const SANDBOX_HINT: Record<string, string> = {
  "read-only": "I comandi possono solo leggere.",
  "workspace-write": "I comandi scrivono solo dentro il workspace.",
  off: "Nessun contenimento dei comandi.",
};

export function SecurityView() {
  const raw = useParams().tab;
  const tab: Tab = TABS.includes(raw as Tab) ? (raw as Tab) : "policy";
  const cfg = useConfig();
  return (
    <Page>
      <PageHeader
        eyebrow="Controllo" title="Sicurezza"
        subtitle="Cosa può fare un agente senza chiedere: regole sui tool, hook di controllo e contenimento dei comandi."
      />
      <Tabs
        idPrefix="sec" label="Sicurezza" value={tab} onChange={(t) => navigate(t === "policy" ? "/security" : `/security/${t}`)}
        tabs={[{ id: "policy", label: "Policy" }, { id: "hooks", label: "Hook" }, { id: "sandbox", label: "Sandbox" }]}
      />
      <TabPanel idPrefix="sec" id={tab}>
        {tab === "policy" && (
          <Section title="Regole sui tool" hint="Si applicano in aggiunta alla modalità di approvazione dell'agente; la prima regola che combacia decide.">
            <PolicyEditor />
          </Section>
        )}
        {tab === "hooks" && (
          <Section title="Hook del ciclo di vita" hint="Comandi di shell che ricevono l'evento in JSON; il codice di uscita 2 blocca l'azione.">
            <HooksEditor />
          </Section>
        )}
        {tab === "sandbox" && (
          <Section title="Modalità e sandbox correnti" hint="Valori effettivi della configurazione a strati; ogni agente può avere i propri.">
            <QueryState query={cfg}>
              {(view) => {
                const mode = String(view.effective["policy.mode"] ?? "ask");
                const sandbox = String(view.effective["sandbox.mode"] ?? "workspace-write");
                return (
                  <Panel>
                    <KeyValue items={[
                      ["Modalità di approvazione", <><Pill tone="info">{mode}</Pill> <span className="muted">{MODE_HINT[mode]}</span></>],
                      ["Sandbox", <><Pill tone={sandbox === "off" ? "crit" : "ok"}>{sandbox}</Pill> <span className="muted">{SANDBOX_HINT[sandbox]}</span></>],
                    ]} />
                    <p className="faint text-sm mt-3">Per cambiarli apri le <Link to="/settings">Impostazioni</Link> (strati globale, device o workspace) oppure l'agente interessato.</p>
                  </Panel>
                );
              }}
            </QueryState>
          </Section>
        )}
      </TabPanel>
    </Page>
  );
}
