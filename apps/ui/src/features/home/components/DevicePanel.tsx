import { Link } from "../../../app/router";
import { formatMb } from "../../../shared/lib/format";
import { useSystem } from "../../../shared/query/system";
import { Chip, Chips, Icon, KeyValue, Led, Meter, Panel, Pill, QueryState } from "../../../shared/ui";

import { t, t as tr } from "../../../shared/i18n";
/** This device: tier, system, memory in use and accelerators. */
export function DevicePanel() {
  const system = useSystem();
  const d = system.data?.device;
  return (
    <Panel title={t("home.questo_dispositivo")} icon="device" actions={d && <Pill tone="ok"><Led tone="ok" small /> {d.tier.toUpperCase()}</Pill>}>
      <QueryState query={system}>
        {({ device }) => {
          const used = device.memory_mb - device.available_memory_mb;
          const pct = device.memory_mb > 0 ? used / device.memory_mb : 0;
          return (
            <div className="stack stack-lg">
              <KeyValue items={[["Nome", <b>{device.name}</b>], ["Sistema", `${device.os_version} · ${device.arch}`], ["CPU", `${device.cpu.brand} · ${device.cpu.threads} thread`]]} />
              <div className="stack stack-sm">
                <div className="row row-between">
                  <span className="text-sm">{t("home.memoria")}</span>
                  <span className="mono faint num text-xs">{formatMb(used)} / {formatMb(device.memory_mb)}</span>
                </div>
                <Meter label={t("home.memoria_in_uso")} value={used} max={device.memory_mb} tone={pct > 0.85 ? "crit" : pct > 0.7 ? "warn" : "ok"} />
              </div>
              <Chips label={t("home.acceleratori")}>
                {device.accelerators.map((a, i) => <Chip key={i} hot={a.kind !== "cpu"} title={a.name}>{a.kind}{a.memory_mb ? ` ${formatMb(a.memory_mb)}` : ""}</Chip>)}
              </Chips>
              <Link to="/devices" className="link-more">{tr("home.dettagli_del_dispositivo")}{" "}<Icon name="forward" size={14} /></Link>
            </div>
          );
        }}
      </QueryState>
    </Panel>
  );
}
