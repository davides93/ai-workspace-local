import { ProfilesPanel } from "../agents/components/ProfilesPanel";
import "../agents/agents.css";
import { navigate, useParams } from "../../app/router";
import { isApplePhoneOrTablet } from "../../shared/lib/platform";
import { formatBytes, formatMb } from "../../shared/lib/format";
import { useSystem } from "../../shared/query/system";
import { Page, PageHeader, QueryState, TabPanel, Tabs } from "../../shared/ui";
import { isActiveDownload, useDownloads, useModels } from "./api";
import { ActiveDownloadsStrip } from "./components/ActiveDownloadsStrip";
import { CatalogTab } from "./components/CatalogTab";
import { GalleryTab } from "./components/GalleryTab";
import { DiscoverTab } from "./components/DiscoverTab";
import { DownloadsTab } from "./components/DownloadsTab";
import { InstalledTab } from "./components/InstalledTab";
import "./models.css";

import { t as tr } from "../../shared/i18n";
type Tab = "gallery" | "installed" | "discover" | "downloads" | "catalog" | "profiles";
const TABS: Tab[] = ["gallery", "installed", "discover", "downloads", "catalog", "profiles"];
/** Tab of `/models`: the gallery on iPhone and iPad, the installed models elsewhere. */
const defaultTab = (): Tab => (isApplePhoneOrTablet() ? "gallery" : "installed");

/** Modelli: installed, live discovery on several providers, the download queue, and the curated catalog. */
export function ModelsView() {
  const models = useModels();
  const downloads = useDownloads();
  const system = useSystem();
  const rawTab = useParams().tab;
  const tab: Tab = TABS.includes(rawTab as Tab) ? (rawTab as Tab) : defaultTab();
  const jobs = downloads.data ?? [];

  const totalInstalled = models.data?.reduce((s, m) => s + m.size_bytes, 0) ?? 0;
  const vram = system.data?.device.accelerators.find((a) => a.memory_mb)?.memory_mb;

  return (
    <Page>
      <PageHeader
        eyebrow="Inferenza" title={tr("models.modelli")}
        subtitle={tr("models.disponibili_su_disco_memoria_disponibile", { p: models.data?.length ?? 0, p2: formatBytes(totalInstalled), p3: formatMb(system.data?.device.memory_mb), p4: vram ? `, VRAM ${formatMb(vram)}` : "" })}
      />
      <Tabs
        idPrefix="models" label={tr("models.sezioni_dei_modelli")} value={tab} onChange={(t) => navigate(t === defaultTab() ? "/models" : `/models/${t}`)}
        tabs={[
          { id: "gallery", label: tr("models.galleria") },
          { id: "installed", label: tr("models.disponibili_qui"), count: models.data?.length },
          { id: "profiles", label: tr("models.profili") },
          { id: "discover", label: tr("models.scopri") },
          { id: "downloads", label: tr("models.download"), count: jobs.filter(isActiveDownload).length || undefined },
          { id: "catalog", label: tr("models.catalogo") },
        ]}
      />
      {tab !== "downloads" && <ActiveDownloadsStrip jobs={jobs} />}
      <TabPanel idPrefix="models" id={tab}>
        {tab === "profiles" && <ProfilesPanel />}
        {tab === "gallery" && <GalleryTab jobs={jobs} />}
        {tab === "installed" && <InstalledTab />}
        {tab === "discover" && <DiscoverTab />}
        {tab === "downloads" && <QueryState query={downloads}>{(list) => <DownloadsTab jobs={list} />}</QueryState>}
        {tab === "catalog" && <CatalogTab jobs={jobs} />}
      </TabPanel>
    </Page>
  );
}
