import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, TabPanel, Tabs, type TabItem } from "../../shared/ui";
import { AdaptersTab } from "./components/AdaptersTab";
import { DatasetsTab } from "./components/DatasetsTab";
import { JobsTab } from "./components/JobsTab";
import { useAdapters, useDatasets, useTrainDoctor, useTrainJobs } from "./api";
import { isActive } from "./model/training";
import "./training.css";

import { t as tr } from "../../shared/i18n";
type Tab = "jobs" | "datasets" | "adapters";

/** `#/training/jobs` (device, trainers and fine-tunings), `#/training/datasets` and `#/training/adapters`. */
export function TrainingView() {
  const p = useParams().tab;
  const tab: Tab = p === "datasets" || p === "adapters" ? p : "jobs";
  const doctor = useTrainDoctor();
  const jobs = useTrainJobs();
  const datasets = useDatasets();
  const adapters = useAdapters();
  const running = (jobs.data ?? []).filter(isActive).length;
  const tabs: TabItem<Tab>[] = [
    { id: "jobs", label: running ? tr("training.addestramenti_in_corso", { running }) : tr("training.addestramenti"), count: jobs.data?.length },
    { id: "datasets", label: tr("training.dataset"), count: datasets.data?.length },
    { id: "adapters", label: tr("training.adapter"), count: adapters.data?.length },
  ];
  return (
    <Page className="tr-page">
      <PageHeader
        eyebrow={tr("nav.group_inference")} title={tr("training.addestramento")}
        subtitle={tr("training.affina_un_modello_locale_con")}
      />
      <Tabs tabs={tabs} value={tab} onChange={(t) => navigate(`/training/${t}`)} label={tr("training.sezioni_dell_addestramento")} idPrefix="tr" />
      <TabPanel idPrefix="tr" id={tab}>
        {tab === "jobs" && <JobsTab doctor={doctor} jobs={jobs} datasets={datasets.data ?? []} />}
        {tab === "datasets" && <DatasetsTab datasets={datasets} doctor={doctor.data} />}
        {tab === "adapters" && <AdaptersTab adapters={adapters} />}
      </TabPanel>
    </Page>
  );
}
