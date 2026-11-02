import { useSystem } from "../../shared/query/system";
import { Page, PageHeader, QueryState, PageSkeleton } from "../../shared/ui";
import { HardwarePanel } from "./components/HardwarePanel";
import { MeshSection } from "./components/MeshSection";
import { TierPanel } from "./components/TierPanel";
import "./devices.css";

/** `#/devices` — this device's hardware profile and the mesh of paired devices. */
export function DevicesView() {
  const system = useSystem();
  return (
    <Page>
      <PageHeader eyebrow="Inferenza" title="Dispositivi" subtitle="Il profilo hardware guida la scelta automatica di runtime e modelli; puoi sempre forzarla." />
      <QueryState query={system} skeleton={<PageSkeleton />}>
        {(info) => (
          <>
            <div className="device-hero">
              <HardwarePanel system={info} />
              <TierPanel tier={info.device.tier} />
            </div>
            <MeshSection />
          </>
        )}
      </QueryState>
    </Page>
  );
}
