import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, Section, TabPanel, Tabs } from "../../shared/ui";
import { HooksEditor } from "./components/HooksEditor";
import { PolicyEditor } from "./components/PolicyEditor";
import { SandboxPanel } from "./components/SandboxPanel";
import "./security.css";

type Tab = "policy" | "hooks" | "sandbox";
const TABS: Tab[] = ["policy", "hooks", "sandbox"];

export function SecurityView() {
  const raw = useParams().tab;
  const tab: Tab = TABS.includes(raw as Tab) ? (raw as Tab) : "policy";
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
      </TabPanel>
    </Page>
  );
}
