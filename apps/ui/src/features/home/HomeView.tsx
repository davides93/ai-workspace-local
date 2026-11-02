import { useState } from "react";
import { useSystem } from "../../shared/query/system";
import { Page, PageHeader } from "../../shared/ui";
import { useAgents, useProfiles } from "../agents/api";
import { useSessions } from "../chat/api";
import { useModels } from "../models/api";
import { useRuntimes } from "../runtimes/api";
import { useConfig } from "../settings/api";
import { ApprovalsPanel } from "./components/ApprovalsPanel";
import { DevicePanel } from "./components/DevicePanel";
import { DownloadsPanel } from "./components/DownloadsPanel";
import { FirstRun } from "./components/FirstRun";
import { ModelPanel } from "./components/ModelPanel";
import { QuickActions } from "./components/QuickActions";
import { RecentSessions } from "./components/RecentSessions";
import { RuntimesPanel } from "./components/RuntimesPanel";
import { checklist, defaultProfile, firstRunPending } from "./model/overview";
import "./home.css";

const DISMISSED_KEY = "aiw.home.firstrun";

function readDismissed(): boolean {
  try {
    return localStorage.getItem(DISMISSED_KEY) === "dismissed";
  } catch {
    return false;
  }
}

/** Panoramica: quick actions, the first-run checklist while something is missing, and the state of the workspace. */
export function HomeView() {
  const system = useSystem();
  const runtimes = useRuntimes();
  const models = useModels();
  const agents = useAgents();
  const profiles = useProfiles();
  const sessions = useSessions();
  const config = useConfig();
  const [dismissed, setDismissed] = useState(readDismissed);

  // The checklist needs every answer: until they are all in, nothing flashes in and out.
  const ready = !!(runtimes.data && models.data && profiles.data && sessions.data);
  const steps = ready ? checklist({ runtimes: runtimes.data!, models: models.data!, profiles: profiles.data!, sessions: sessions.data!.length }) : [];
  const showFirstRun = ready && !dismissed && firstRunPending(steps);
  const profile = defaultProfile(agents.data ?? [], profiles.data ?? [], config.data?.effective["agent.default"]);

  const dismiss = () => {
    setDismissed(true);
    try {
      localStorage.setItem(DISMISSED_KEY, "dismissed");
    } catch {
      /* storage unavailable: hidden until the page is reloaded */
    }
  };

  return (
    <Page className="hm">
      <PageHeader eyebrow="Panoramica" title="Il tuo workspace" subtitle="Dispositivo, runtime, modelli e lavoro in corso, a colpo d'occhio." />
      <QuickActions />
      {showFirstRun && <FirstRun steps={steps} runtimes={runtimes.data!} models={models.data!} profile={profile} device={system.data?.device} onDismiss={dismiss} />}
      <div className="hm-grid">
        <DevicePanel />
        <RuntimesPanel />
        <ModelPanel />
        <ApprovalsPanel />
        <RecentSessions />
        <DownloadsPanel />
      </div>
    </Page>
  );
}
