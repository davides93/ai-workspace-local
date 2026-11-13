import { useSystem } from "../../shared/query/system";
import { Page, PageHeader, QueryState, PageSkeleton } from "../../shared/ui";
import { AccessPanel } from "../security/components/AccessPanel";
import { HardwarePanel } from "./components/HardwarePanel";
import { MeshSection } from "./components/MeshSection";
import { TierPanel } from "./components/TierPanel";
import "./devices.css";

import { t } from "../../shared/i18n";
/** `#/devices` — this device's hardware profile and the mesh of paired devices. */
export function DevicesView() {
  const system = useSystem();
  return (
    <Page>
      <PageHeader eyebrow="Inferenza" title={t("devices.dispositivi")} subtitle={t("devices.il_profilo_hardware_guida_la")} />
      <QueryState query={system} skeleton={<PageSkeleton />}>
        {(info) => (
          <>
            <div className="device-hero">
              <HardwarePanel system={info} />
              <TierPanel tier={info.device.tier} />
            </div>
            <MeshSection />
            <AccessPanel />
          </>
        )}
      </QueryState>
    </Page>
  );
}
