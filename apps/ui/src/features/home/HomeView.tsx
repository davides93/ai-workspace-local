import { useState } from "react";
import { useUiMode } from "../../app/useUiMode";
import { useSystem } from "../../shared/query/system";
import { Page, PageHeader } from "../../shared/ui";
import { useAgents, useProfiles } from "../agents/api";
import { useSessions } from "../chat/api";
import { useCatalog, useModels } from "../models/api";
import { useRuntimes } from "../runtimes/api";
import { LessonsPanel } from "../learning/components/LessonsPanel";
import { useConfig } from "../settings/api";
import { useApprovals } from "../approvals/api";
import { useLessons } from "../learning/api";
import { AssistantCard } from "./components/AssistantCard";
import { ApprovalsPanel } from "./components/ApprovalsPanel";
import { DevicePanel } from "./components/DevicePanel";
import { DownloadsPanel } from "./components/DownloadsPanel";
import { FirstRun } from "./components/FirstRun";
import { SimpleFirstRunPanel } from "./components/SimpleFirstRun";
import { ModelPanel } from "./components/ModelPanel";
import { QuickActions } from "./components/QuickActions";
import { RecentSessions } from "./components/RecentSessions";
import { RuntimesPanel } from "./components/RuntimesPanel";
import { assistantModelName, simpleSections } from "./model/simple";
import { checklist, defaultProfile, firstRunPending, simpleFirstRun, simpleFirstRunPending } from "./model/overview";
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
  const catalog = useCatalog();
  const approvals = useApprovals();
  const lessons = useLessons("pending");
  const [mode] = useUiMode();
  const [dismissed, setDismissed] = useState(readDismissed);

  // The checklist needs every answer: until they are all in, nothing flashes in and out.
  const ready = !!(runtimes.data && models.data && profiles.data && sessions.data);
  const steps = ready ? checklist({ runtimes: runtimes.data!, models: models.data!, profiles: profiles.data!, sessions: sessions.data!.length }) : [];
  const profile = defaultProfile(agents.data ?? [], profiles.data ?? [], config.data?.effective["agent.default"]);
  // Simple mode: one panel instead of the checklist; it also waits for the agents, whose default profile it may fix.
  const simple = ready && agents.data ? simpleFirstRun({ runtimes: runtimes.data!, models: models.data!, profile, catalog: catalog.data }) : undefined;
  const showFirstRun = ready && !dismissed && (mode === "simple" ? !!simple && simpleFirstRunPending(simple, sessions.data!.length) : firstRunPending(steps));

  const modelName = assistantModelName(profile, models.data ?? []);
  const show = simpleSections({ modelName, approvals: approvals.data?.length ?? 0, sessions: sessions.data?.length ?? 0, lessons: lessons.data?.length ?? 0 });

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
      <PageHeader eyebrow="Panoramica" title="Il tuo workspace" subtitle={mode === "simple" ? "Il tuo assistente e il lavoro in corso, a colpo d'occhio." : "Dispositivo, runtime, modelli e lavoro in corso, a colpo d'occhio."} />
      <QuickActions simple={mode === "simple"} />
      {showFirstRun && mode === "simple" && simple && (
        <SimpleFirstRunPanel state={simple} models={models.data!} runtimes={runtimes.data!} profile={profile} device={system.data?.device} onDismiss={dismiss} />
      )}
      {showFirstRun && mode === "developer" && <FirstRun steps={steps} runtimes={runtimes.data!} models={models.data!} profile={profile} device={system.data?.device} onDismiss={dismiss} />}
      {mode === "simple" ? (
        <div className="hm-grid">
          {show.assistant && <AssistantCard name={modelName!} />}
          {show.approvals && <ApprovalsPanel />}
          {show.recent && <RecentSessions simple />}
          {show.lessons && <LessonsPanel />}
        </div>
      ) : (
      <div className="hm-grid">
        <DevicePanel />
        <RuntimesPanel />
        <ModelPanel />
        <ApprovalsPanel />
        <LessonsPanel />
        <RecentSessions />
        <DownloadsPanel />
      </div>
      )}
    </Page>
  );
}
