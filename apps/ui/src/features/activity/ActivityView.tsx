import { useState } from "react";
import { formatDateTime } from "../../shared/lib/format";
import { DataTable, Page, PageHeader, Pill, QueryState, Segmented, type Column, type PillTone } from "../../shared/ui";
import type { AuditEntry } from "../../shared/api/types";
import { useAudit } from "./api";

const TONE: Record<string, PillTone> = { allow: "ok", approved: "ok", ask: "warn", deny: "crit", rejected: "crit" };
const ACTORS = [{ id: "all", label: "Tutto" }, { id: "agent", label: "Agenti" }, { id: "user", label: "Utente" }, { id: "workflow", label: "Workflow" }, { id: "system", label: "Sistema" }];

const columns: Column<AuditEntry>[] = [
  { key: "ts", header: "Quando", className: "num mono text-xs", nowrap: true, render: (a) => formatDateTime(a.ts) },
  { key: "actor", header: "Attore", render: (a) => a.actor },
  { key: "action", header: "Azione", className: "mono", render: (a) => (a.tool ? `${a.action}:${a.tool}` : a.action) },
  { key: "decision", header: "Decisione", render: (a) => (a.decision ? <Pill tone={TONE[a.decision] ?? "neutral"}>{a.decision}</Pill> : "—") },
  { key: "detail", header: "Dettaglio", className: "mono text-xs cell-detail", render: (a) => a.detail },
  { key: "run", header: "Run", className: "mono faint text-xs", render: (a) => a.run_id ?? "—" },
];

export function ActivityView() {
  const audit = useAudit();
  const [actor, setActor] = useState("all");
  return (
    <Page>
      <PageHeader
        eyebrow="Controllo" title="Attività"
        subtitle="Audit trail di ogni decisione di policy, azione dei tool e comando di sistema."
        actions={<Segmented label="Filtro attore" value={actor} onChange={setActor} options={ACTORS} />}
      />
      <QueryState query={audit}>
        {(all) => (
          <DataTable label="Audit" columns={columns} rows={all.filter((a) => actor === "all" || a.actor === actor)} rowKey={(a) => a.id} empty="Ancora nessuna attività." />
        )}
      </QueryState>
    </Page>
  );
}
