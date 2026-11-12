import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, Section, TabPanel, Tabs } from "../../shared/ui";
import { AccessPanel } from "./components/AccessPanel";
import { HooksEditor } from "./components/HooksEditor";
import { NetworkPanel } from "./components/NetworkPanel";
import { PolicyEditor } from "./components/PolicyEditor";
import { SandboxPanel } from "./components/SandboxPanel";
import "./security.css";

type Tab = "policy" | "hooks" | "sandbox" | "access" | "network";
const TABS: Tab[] = ["policy", "hooks", "sandbox", "access", "network"];

export function SecurityView() {
  const raw = useParams().tab;
  const tab: Tab = TABS.includes(raw as Tab) ? (raw as Tab) : "policy";
  return (
    <Page>
      <PageHeader
        eyebrow="Controllo" title="Sicurezza"
        subtitle="Cosa può fare un agente senza chiedere: regole sui tool, hook di controllo, contenimento dei comandi, accessi dei dispositivi e cifratura della rete."
      />
      <Tabs
        idPrefix="sec" label="Sicurezza" value={tab} onChange={(t) => navigate(t === "policy" ? "/security" : `/security/${t}`)}
        tabs={[{ id: "policy", label: "Policy" }, { id: "hooks", label: "Hook" }, { id: "sandbox", label: "Sandbox" }, { id: "access", label: "Accessi" }, { id: "network", label: "Rete" }]}
      />
      <TabPanel idPrefix="sec" id={tab}>
        {tab === "policy" && (
          <Section title="Regole sui tool" hint="Valgono per tutti gli agenti, sopra la loro modalità di approvazione.">
            <PolicyEditor />
          </Section>
        )}
        {tab === "hooks" && (
          <Section title="Hook del ciclo di vita" hint="Comandi di shell che ricevono l'evento in JSON e possono bloccarlo.">
            <HooksEditor />
          </Section>
        )}
        {tab === "sandbox" && <SandboxPanel />}
        {tab === "access" && (
          <Section title="Accessi" hint="Chi può usare questo computer dalla rete: ogni dispositivo ha il suo token e si può scollegare da solo.">
            <AccessPanel />
          </Section>
        )}
        {tab === "network" && (
          <Section title="Rete" hint="Come questo computer è raggiungibile e se il traffico è cifrato.">
            <NetworkPanel />
          </Section>
        )}
      </TabPanel>
    </Page>
  );
}
