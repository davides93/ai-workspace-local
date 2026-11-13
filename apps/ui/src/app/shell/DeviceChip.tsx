import { formatMb } from "../../shared/lib/format";
import { useSystem } from "../../shared/query/system";
import { Led } from "../../shared/ui";
import { Link } from "../router";

import { t } from "../../shared/i18n";
/** Current device with its main accelerator; opens the Devices page. */
export function DeviceChip({ onNavigate }: { onNavigate?: () => void }) {
  const { data: system } = useSystem();
  const accel = system?.device.accelerators.find((a) => a.kind !== "cpu");
  return (
    <Link to="/devices" className="device-chip" title={t("shell.dispositivo_corrente")} aria-label={t("shell.dispositivo_corrente_2", { name: system?.device.name ?? t("shell.detecting_device") })} onClick={onNavigate}>
      <Led tone={system ? "ok" : "off"} />
      <b className="ellipsis chip-text">{system?.device.name ?? t("shell.rilevamento")}</b>
      <span className="muted mono ellipsis chip-text text-xs">
        {t("shell.ram", { accel: accel ? `${accel.kind.toUpperCase()} ${formatMb(accel.memory_mb)}` : system ? "CPU" : "", mem: formatMb(system?.device.memory_mb) })}</span>
    </Link>
  );
}
