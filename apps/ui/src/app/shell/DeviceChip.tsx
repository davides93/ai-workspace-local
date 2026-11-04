import { formatMb } from "../../shared/lib/format";
import { useSystem } from "../../shared/query/system";
import { Led } from "../../shared/ui";
import { Link } from "../router";

/** Current device with its main accelerator; opens the Devices page. */
export function DeviceChip({ onNavigate }: { onNavigate?: () => void }) {
  const { data: system } = useSystem();
  const accel = system?.device.accelerators.find((a) => a.kind !== "cpu");
  return (
    <Link to="/devices" className="device-chip" title="Dispositivo corrente" aria-label={`Dispositivo corrente: ${system?.device.name ?? "rilevamento in corso"}`} onClick={onNavigate}>
      <Led tone={system ? "ok" : "off"} />
      <b className="ellipsis chip-text">{system?.device.name ?? "Rilevamento…"}</b>
      <span className="muted mono ellipsis chip-text text-xs">
        {accel ? `${accel.kind.toUpperCase()} ${formatMb(accel.memory_mb)}` : system ? "CPU" : ""} · RAM {formatMb(system?.device.memory_mb)}
      </span>
    </Link>
  );
}
