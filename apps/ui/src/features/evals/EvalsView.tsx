import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, TabPanel, Tabs, type TabItem } from "../../shared/ui";
import { OptimizationsTab } from "./components/OptimizationsTab";
import { ReportsTab } from "./components/ReportsTab";
import { SuitesTab } from "./components/SuitesTab";
import { useEvalReports, useEvalSuites, useOptimizations } from "./api";
import { isRunning } from "./model/evals";
import { isOptimizing } from "./model/optimizations";
import "./evals.css";

type Tab = "suites" | "reports" | "optimizations";

/**
 * `#/evals/suites` (the suites: edit, run, optimize the prompt), `#/evals/reports` (what the runs found, and A/B comparison)
 * and `#/evals/optimizations` (the prompt variants tried on a suite, to apply to the agent or not).
 */
export function EvalsView() {
  const param = useParams().tab;
  const tab: Tab = param === "reports" || param === "optimizations" ? param : "suites";
  const suites = useEvalSuites();
  const reports = useEvalReports();
  const optimizations = useOptimizations();
  const running = (reports.data ?? []).filter(isRunning).length;
  const optimizing = (optimizations.data ?? []).filter(isOptimizing).length;
  const tabs: TabItem<Tab>[] = [
    { id: "suites", label: "Suite", count: suites.data?.length },
    { id: "reports", label: running ? `Report · ${running} in corso` : "Report", count: reports.data?.length },
    { id: "optimizations", label: optimizing ? `Ottimizzazioni · ${optimizing} in corso` : "Ottimizzazioni", count: optimizations.data?.length },
  ];
  return (
    <Page className="ev-page">
      <PageHeader
        eyebrow="Lavoro" title="Valutazioni"
        subtitle="Dai a un agente dei task con file di partenza e controlla se li ha svolti: grader sui file, sulla risposta o un modello come giudice. Confronta due esecuzioni."
      />
      <Tabs tabs={tabs} value={tab} onChange={(t) => navigate(`/evals/${t}`)} label="Sezioni delle valutazioni" idPrefix="ev" />
      <TabPanel idPrefix="ev" id={tab}>
        {tab === "suites" ? <SuitesTab suites={suites} /> : tab === "reports" ? <ReportsTab reports={reports} suites={suites.data ?? []} /> : <OptimizationsTab optimizations={optimizations} suites={suites.data ?? []} />}
      </TabPanel>
    </Page>
  );
}
