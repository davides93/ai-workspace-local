import { Link } from "../../../app/router";
import type { Accelerator, SystemInfo } from "../../../shared/api/types";
import { formatMb } from "../../../shared/lib/format";
import { Chip, Chips, DataTable, Icon, KeyValue, Led, Meter, Pill, type Column } from "../../../shared/ui";
import { budgetRule, memoryUsage, modelBudgetMb, vramMb } from "../model/hardware";

const ACCEL_COLUMNS: Column<Accelerator>[] = [
  { key: "kind", header: "Tipo", nowrap: true, render: (a) => <Chip hot>{a.kind}</Chip> },
  {
    key: "name", header: "Dispositivo",
    render: (a) => (
      <>
        <div>{a.name}</div>
        {[a.vendor, a.driver, a.details].some(Boolean) && <div className="faint mono text-xs">{[a.vendor, a.driver, a.details].filter(Boolean).join(" · ")}</div>}
      </>
    ),
  },
  { key: "mem", header: "Memoria", className: "num mono", nowrap: true, render: (a) => (a.memory_mb ? formatMb(a.memory_mb) : "—") },
];

/** Memory of this device and the share of it that models can use. */
function MemoryBudget({ device: d }: { device: SystemInfo["device"] }) {
  const mem = memoryUsage(d);
  const budget = modelBudgetMb(d);
  return (
    <section className="stack stack-lg" aria-label="Memoria">
      <div className="stack stack-sm">
        <div className="row row-between">
          <b className="text-sm">RAM in uso</b>
          <span className="mono faint num text-xs">{formatMb(mem.used)} usati di {formatMb(d.memory_mb)}</span>
        </div>
        <Meter label="RAM in uso" value={mem.used} max={d.memory_mb} tone={mem.tone} />
      </div>
      <div className="stack stack-sm">
        <div className="row row-between">
          <b className="text-sm">Budget per i modelli</b>
          <span className="mono num text-xs">{formatMb(budget)}</span>
        </div>
        <Meter label="Budget per i modelli" value={budget} max={Math.max(d.memory_mb, vramMb(d), budget)} />
        <p className="faint text-xs">{budgetRule(d)} <Link to="/models/catalog" className="link-more">Modelli che ci stanno <Icon name="forward" size={13} /></Link></p>
      </div>
    </section>
  );
}

/** This device: CPU and paths, memory budget and the accelerators table. */
export function HardwarePanel({ system }: { system: SystemInfo }) {
  const d = system.device;
  return (
    <div className="panel">
      <div className="panel-head">
        <Icon name="device" />
        <div className="grow">
          <h2>{d.name}</h2>
          <span className="muted text-sm">{d.os_version} · {d.arch} · {d.form_factor}</span>
        </div>
        <Pill tone="ok"><Led tone="ok" small /> questo device</Pill>
      </div>
      <div className="panel-body stack stack-lg">
        <KeyValue items={[
          ["CPU", `${d.cpu.brand} · ${d.cpu.cores} core / ${d.cpu.threads} thread`],
          ["Istruzioni", <Chips label="Istruzioni della CPU">{d.cpu.features.map((f) => <Chip key={f}>{f}</Chip>)}</Chips>],
          ["Dati", <span className="mono">{system.data_dir}</span>],
          ["Workspace", <span className="mono">{system.workspace_root}</span>],
        ]} />
        <MemoryBudget device={d} />
        <section className="stack stack-sm" aria-label="Acceleratori">
          <b className="text-sm">Acceleratori</b>
          <DataTable label="Acceleratori" columns={ACCEL_COLUMNS} rows={d.accelerators} rowKey={(a) => `${a.kind}-${a.name}`} empty="Nessun acceleratore rilevato: i modelli girano su CPU." />
        </section>
      </div>
    </div>
  );
}
