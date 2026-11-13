import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, Section, TabPanel, Tabs } from "../../shared/ui";
import { AccessPanel } from "./components/AccessPanel";
import { HooksEditor } from "./components/HooksEditor";
import { NetworkPanel } from "./components/NetworkPanel";
import { PolicyEditor } from "./components/PolicyEditor";
import { SandboxPanel } from "./components/SandboxPanel";
import "./security.css";

import { t as tr } from "../../shared/i18n";
type Tab = "policy" | "hooks" | "sandbox" | "access" | "network";
const TABS: Tab[] = ["policy", "hooks", "sandbox", "access", "network"];

export function SecurityView() {
  const raw = useParams().tab;
  const tab: Tab = TABS.includes(raw as Tab) ? (raw as Tab) : "policy";
  return (
    <Page>
      <PageHeader
        eyebrow={tr("nav.group_control")} title={tr("security.sicurezza")}
        subtitle={tr("security.cosa_puo_fare_un_agente")}
      />
      <Tabs
        idPrefix="sec" label={tr("security.sicurezza")} value={tab} onChange={(t) => navigate(t === "policy" ? "/security" : `/security/${t}`)}
        tabs={[{ id: "policy", label: tr("security.policy") }, { id: "hooks", label: tr("security.hook") }, { id: "sandbox", label: tr("security.sandbox") }, { id: "access", label: tr("security.accessi") }, { id: "network", label: tr("security.rete") }]}
      />
      <TabPanel idPrefix="sec" id={tab}>
        {tab === "policy" && (
          <Section title={tr("security.regole_sui_tool")} hint={tr("security.valgono_per_tutti_gli_agenti")}>
            <PolicyEditor />
          </Section>
        )}
        {tab === "hooks" && (
          <Section title={tr("security.hook_del_ciclo_di_vita")} hint={tr("security.comandi_di_shell_che_ricevono")}>
            <HooksEditor />
          </Section>
        )}
        {tab === "sandbox" && <SandboxPanel />}
        {tab === "access" && (
          <Section title={tr("security.accessi")} hint={tr("security.chi_puo_usare_questo_computer")}>
            <AccessPanel />
          </Section>
        )}
        {tab === "network" && (
          <Section title={tr("security.rete")} hint={tr("security.come_questo_computer_e_raggiungibile")}>
            <NetworkPanel />
          </Section>
        )}
      </TabPanel>
    </Page>
  );
}
