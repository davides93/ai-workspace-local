import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, TabPanel, Tabs, type TabItem } from "../../shared/ui";
import { ReportsTab } from "./components/ReportsTab";
import { SuitesTab } from "./components/SuitesTab";
import { useEvalReports, useEvalSuites } from "./api";
import { isRunning } from "./model/evals";
import "./evals.css";

type Tab = "suites" | "reports";

/** `#/evals/suites` (the suites: edit, run) and `#/evals/reports` (what the runs found, and A/B comparison). */
export function EvalsView() {
  const tab: Tab = useParams().tab === "reports" ? "reports" : "suites";
  const suites = useEvalSuites();
  const reports = useEvalReports();
  const running = (reports.data ?? []).filter(isRunning).length;
  const tabs: TabItem<Tab>[] = [
    { id: "suites", label: "Suite", count: suites.data?.length },
    { id: "reports", label: running ? `Report · ${running} in corso` : "Report", count: reports.data?.length },
  ];
  return (
    <Page className="ev-page">
      <PageHeader
        eyebrow="Lavoro" title="Valutazioni"
        subtitle="Dai a un agente dei task con file di partenza e controlla se li ha svolti: grader sui file, sulla risposta o un modello come giudice. Confronta due esecuzioni."
      />
      <Tabs tabs={tabs} value={tab} onChange={(t) => navigate(`/evals/${t}`)} label="Sezioni delle valutazioni" idPrefix="ev" />
      <TabPanel idPrefix="ev" id={tab}>
        {tab === "suites" ? <SuitesTab suites={suites} /> : <ReportsTab reports={reports} suites={suites.data ?? []} />}
      </TabPanel>
    </Page>
  );
}
