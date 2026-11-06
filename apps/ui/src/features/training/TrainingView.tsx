import { navigate, useParams } from "../../app/router";
import { Page, PageHeader, TabPanel, Tabs, type TabItem } from "../../shared/ui";
import { AdaptersTab } from "./components/AdaptersTab";
import { DatasetsTab } from "./components/DatasetsTab";
import { JobsTab } from "./components/JobsTab";
import { useAdapters, useDatasets, useTrainDoctor, useTrainJobs } from "./api";
import { isActive } from "./model/training";
import "./training.css";

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
    { id: "jobs", label: running ? `Addestramenti · ${running} in corso` : "Addestramenti", count: jobs.data?.length },
    { id: "datasets", label: "Dataset", count: datasets.data?.length },
    { id: "adapters", label: "Adapter", count: adapters.data?.length },
  ];
  return (
    <Page className="tr-page">
      <PageHeader
        eyebrow="Inferenza" title="Addestramento"
        subtitle="Affina un modello locale con LoRA sulle tue conversazioni approvate. L'addestramento lo fa un programma esterno (MLX LM, TRL, Unsloth); AIW prepara i dati, lo segue e registra l'adapter."
      />
      <Tabs tabs={tabs} value={tab} onChange={(t) => navigate(`/training/${t}`)} label="Sezioni dell'addestramento" idPrefix="tr" />
      <TabPanel idPrefix="tr" id={tab}>
        {tab === "jobs" && <JobsTab doctor={doctor} jobs={jobs} datasets={datasets.data ?? []} />}
        {tab === "datasets" && <DatasetsTab datasets={datasets} doctor={doctor.data} />}
        {tab === "adapters" && <AdaptersTab adapters={adapters} />}
      </TabPanel>
    </Page>
  );
}
