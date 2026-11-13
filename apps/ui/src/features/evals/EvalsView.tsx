import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, TabPanel, Tabs, type TabItem } from "../../shared/ui";
import { OptimizationsTab } from "./components/OptimizationsTab";
import { ReportsTab } from "./components/ReportsTab";
import { SuitesTab } from "./components/SuitesTab";
import { useEvalReports, useEvalSuites, useOptimizations } from "./api";
import { isRunning } from "./model/evals";
import { isOptimizing } from "./model/optimizations";
import "./evals.css";

import { t as tr } from "../../shared/i18n";
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
    { id: "suites", label: tr("evals.suite"), count: suites.data?.length },
    { id: "reports", label: running ? tr("evals.report_in_corso", { running }) : tr("evals.report"), count: reports.data?.length },
    { id: "optimizations", label: optimizing ? tr("evals.ottimizzazioni_in_corso", { optimizing }) : tr("evals.ottimizzazioni"), count: optimizations.data?.length },
  ];
  return (
    <Page className="ev-page">
      <PageHeader
        eyebrow={tr("nav.group_work")} title={tr("evals.valutazioni")}
        subtitle={tr("evals.dai_a_un_agente_dei")}
      />
      <Tabs tabs={tabs} value={tab} onChange={(t) => navigate(`/evals/${t}`)} label={tr("evals.sezioni_delle_valutazioni")} idPrefix="ev" />
      <TabPanel idPrefix="ev" id={tab}>
        {tab === "suites" ? <SuitesTab suites={suites} /> : tab === "reports" ? <ReportsTab reports={reports} suites={suites.data ?? []} /> : <OptimizationsTab optimizations={optimizations} suites={suites.data ?? []} />}
      </TabPanel>
    </Page>
  );
}
