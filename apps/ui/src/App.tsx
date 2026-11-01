import { useState } from "react";
import { useApp, type Route } from "./state/app";
import { Icon, formatMb } from "./components/ui";
import { ChatView } from "./views/Chat";
import { AgentsView } from "./views/Agents";
import { WorkflowsView } from "./views/Workflows";
import { RuntimesView } from "./views/Runtimes";
import { ModelsView } from "./views/Models";
import { DevicesView } from "./views/Devices";
import { ToolsView } from "./views/Tools";
import { ActivityView } from "./views/Activity";
import { PreferencesView } from "./views/Preferences";
import { ApprovalsCenter } from "./components/Approvals";
import { PairingScreen } from "./components/Pairing";

const NAV: { group: string; items: { id: Route; label: string; icon: string }[] }[] = [
  { group: "Lavoro", items: [
    { id: "chat", label: "Workspace", icon: "chat" },
    { id: "agents", label: "Agenti", icon: "agent" },
    { id: "workflows", label: "Workflow", icon: "flow" },
  ] },
  { group: "Inferenza", items: [
    { id: "runtimes", label: "Runtime", icon: "chip" },
    { id: "models", label: "Modelli", icon: "cube" },
    { id: "devices", label: "Dispositivi", icon: "device" },
  ] },
  { group: "Controllo", items: [
    { id: "tools", label: "Tool e MCP", icon: "tool" },
    { id: "activity", label: "Attività", icon: "activity" },
    { id: "preferences", label: "Impostazioni", icon: "settings" },
  ] },
];

const TABBAR: { id: Route; label: string; icon: string }[] = [
  { id: "chat", label: "Workspace", icon: "chat" },
  { id: "agents", label: "Agenti", icon: "agent" },
  { id: "runtimes", label: "Runtime", icon: "chip" },
  { id: "models", label: "Modelli", icon: "cube" },
  { id: "preferences", label: "Altro", icon: "more" },
];

export function App() {
  const { route, go, system, api, toast, notice, needsPairing } = useApp();
  const [killing, setKilling] = useState(false);
  const accel = system?.device.accelerators.find((a) => a.kind !== "cpu");

  const kill = async () => {
    setKilling(true);
    try {
      await api.kill();
      toast("Kill switch: run, sidecar avviati dai run e browser fermati");
    } catch (e) {
      toast(`Kill switch non riuscito: ${String(e)}`, "crit");
    } finally {
      setKilling(false);
    }
  };

  return (
    <div className="shell">
      <nav className="rail" aria-label="Sezioni">
        <div className="brand">
          <div className="brand-mark"><Icon name="bolt" size={18} /></div>
          <div>
            <div className="brand-name">AI Workspace</div>
            <div className="brand-sub">locale · {system?.version ?? "…"}</div>
          </div>
        </div>
        <button className="device-chip" onClick={() => go("devices")} title="Dispositivo corrente">
          <span className={`led ${system ? "ok" : "off"}`} />
          <b className="ellipsis">{system?.device.name ?? "Rilevamento…"}</b>
          <span className="muted mono ellipsis" style={{ fontSize: "0.72rem" }}>
            {accel ? `${accel.kind.toUpperCase()} ${formatMb(accel.memory_mb)}` : system ? "CPU" : ""} · RAM {formatMb(system?.device.memory_mb)}
          </span>
        </button>
        {NAV.map((g) => (
          <div className="nav-group" key={g.group}>
            <div className="nav-label eyebrow">{g.group}</div>
            {g.items.map((it) => (
              <button key={it.id} className="nav-item" aria-current={route === it.id ? "page" : undefined} onClick={() => go(it.id)}>
                <Icon name={it.icon} />
                {it.label}
              </button>
            ))}
          </div>
        ))}
        <div className="rail-foot">
          <button className="kill" onClick={kill} disabled={killing} title="Ferma subito tutti i run, i sidecar avviati dai run e il browser controllato">
            <Icon name="power" /> Kill switch
          </button>
        </div>
      </nav>

      <div className="main">
        {(notice || system?.mode === "demo") && (
          <div className="banner" role="status">
            <Icon name="sparkle" size={16} />
            <span><b>Demo interattiva</b> · {notice ?? <>dati di esempio; avvia <code>aiw serve</code> o apri l'app desktop per usare i tuoi runtime e modelli</>}</span>
          </div>
        )}
        <ApprovalsCenter />
        {needsPairing && <PairingScreen />}
        {!needsPairing && route === "chat" && <ChatView />}
        {!needsPairing && (
          <>
            {route === "agents" && <AgentsView />}
            {route === "workflows" && <WorkflowsView />}
            {route === "runtimes" && <RuntimesView />}
            {route === "models" && <ModelsView />}
            {route === "devices" && <DevicesView />}
            {route === "tools" && <ToolsView />}
            {route === "activity" && <ActivityView />}
            {route === "preferences" && <PreferencesView />}
          </>
        )}
      </div>

      <nav className="tabbar" aria-label="Sezioni">
        {TABBAR.map((t) => (
          <button key={t.id} aria-current={route === t.id ? "page" : undefined} onClick={() => go(t.id)}>
            <Icon name={t.icon} size={20} />
            {t.label}
          </button>
        ))}
      </nav>
    </div>
  );
}
