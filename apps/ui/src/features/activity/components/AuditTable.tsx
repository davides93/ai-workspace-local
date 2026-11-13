import type { ReactNode } from "react";
import { Link } from "../../../app/router";
import { formatDateTime } from "../../../shared/lib/format";
import { chatPathForRun } from "../../../shared/lib/runIndex";
import { Icon, Pill } from "../../../shared/ui";
import type { AuditEntry } from "../../../shared/api/types";
import { ACTOR_LABEL, decisionInfo } from "../model/filter";

import { t, t as tr } from "../../../shared/i18n";
function RunCell({ runId }: { runId?: string }) {
  if (!runId) return <>—</>;
  const to = chatPathForRun(runId);
  const label = <span className="act-run-id" title={runId}>{runId}</span>;
  return to ? (
    <Link to={to} className="act-run-link" title={t("activity.apri_la_sessione_di_questo")}>
      {label}
      <Icon name="forward" size={12} />
      <span className="sr-only">{t("activity.apri_la_sessione_di_questo")}</span>
    </Link>
  ) : label;
}

/**
 * The audit rows as a table. On phones every row becomes a small card (the header row is hidden and
 * each cell carries its label), so nothing scrolls sideways.
 */
export function AuditTable({ rows, empty }: { rows: AuditEntry[]; empty: ReactNode }) {
  return (
    <div className="table-wrap act-wrap">
      <table className="data act-table" aria-label={t("activity.audit_trail")}>
        <thead>
          <tr>
            <th scope="col">{t("activity.quando")}</th>
            <th scope="col">{t("activity.attore")}</th>
            <th scope="col">{t("activity.azione")}</th>
            <th scope="col">{t("activity.decisione")}</th>
            <th scope="col">{t("activity.dettaglio")}</th>
            <th scope="col">{t("activity.run")}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((a) => {
            const d = a.decision ? decisionInfo(a.decision) : null;
            return (
              <tr key={a.id}>
                <td data-label={tr("activity.quando")} className="num act-when"><time dateTime={a.ts}>{formatDateTime(a.ts)}</time></td>
                <td data-label={tr("activity.attore")}>{ACTOR_LABEL[a.actor] ?? a.actor}</td>
                <td data-label={tr("activity.azione")} className="mono act-action">{a.tool ? `${a.action}:${a.tool}` : a.action}</td>
                <td data-label={tr("activity.decisione")}>{d ? <Pill tone={d.tone}>{d.label}</Pill> : <span className="faint">—</span>}</td>
                <td data-label={tr("activity.dettaglio")} className="mono text-xs act-detail">{a.detail || "—"}</td>
                <td data-label={tr("activity.run")} className="mono text-xs faint"><RunCell runId={a.run_id} /></td>
              </tr>
            );
          })}
          {rows.length === 0 && (
            <tr className="act-empty-row">
              <td colSpan={6} className="table-empty">{empty}</td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
