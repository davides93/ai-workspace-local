import { navigate, useParams } from "../../app/router";
import { formatBytes, formatMb } from "../../shared/lib/format";
import { useSystem } from "../../shared/query/system";
import { Page, PageHeader, QueryState, TabPanel, Tabs } from "../../shared/ui";
import { isActiveDownload, useDownloads, useModels } from "./api";
import { ActiveDownloadsStrip } from "./components/ActiveDownloadsStrip";
import { CatalogTab } from "./components/CatalogTab";
import { DiscoverTab } from "./components/DiscoverTab";
import { DownloadsTab } from "./components/DownloadsTab";
import { InstalledTab } from "./components/InstalledTab";
import "./models.css";

type Tab = "installed" | "discover" | "downloads" | "catalog";
const TABS: Tab[] = ["installed", "discover", "downloads", "catalog"];

/** Modelli: installed, live discovery on several providers, the download queue, and the curated catalog. */
export function ModelsView() {
  const models = useModels();
  const downloads = useDownloads();
  const system = useSystem();
  const rawTab = useParams().tab;
  const tab: Tab = TABS.includes(rawTab as Tab) ? (rawTab as Tab) : "installed";
  const jobs = downloads.data ?? [];

  const totalInstalled = models.data?.reduce((s, m) => s + m.size_bytes, 0) ?? 0;
  const vram = system.data?.device.accelerators.find((a) => a.memory_mb)?.memory_mb;

  return (
    <Page>
      <PageHeader
        eyebrow="Inferenza" title="Modelli"
        subtitle={`${models.data?.length ?? 0} installati · ${formatBytes(totalInstalled)} su disco · memoria disponibile: RAM ${formatMb(system.data?.device.memory_mb)}${vram ? `, VRAM ${formatMb(vram)}` : ""}`}
      />
      <Tabs
        idPrefix="models" label="Sezioni dei modelli" value={tab} onChange={(t) => navigate(t === "installed" ? "/models" : `/models/${t}`)}
        tabs={[
          { id: "installed", label: "Installati", count: models.data?.length },
          { id: "discover", label: "Scopri" },
          { id: "downloads", label: "Download", count: jobs.filter(isActiveDownload).length || undefined },
          { id: "catalog", label: "Catalogo" },
        ]}
      />
      {tab !== "downloads" && <ActiveDownloadsStrip jobs={jobs} />}
      <TabPanel idPrefix="models" id={tab}>
        {tab === "installed" && <InstalledTab />}
        {tab === "discover" && <DiscoverTab />}
        {tab === "downloads" && <QueryState query={downloads}>{(list) => <DownloadsTab jobs={list} />}</QueryState>}
        {tab === "catalog" && <CatalogTab jobs={jobs} />}
      </TabPanel>
    </Page>
  );
}
