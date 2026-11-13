import { useId } from "react";
import { Button, Icon, Segmented, Select } from "../../../shared/ui";
import { ACTOR_LABEL, ALL, decisionInfo, isFiltering, type AuditFilters } from "../model/filter";

import { t as tr } from "../../../shared/i18n";
const ACTORS = [{ id: ALL, label: "Tutto" }, ...Object.entries(ACTOR_LABEL).map(([id, label]) => ({ id, label: id === "agent" ? tr("activity.agenti") : label }))];

/** Filters of the audit trail: actor, decision, tool and free text. */
export function AuditFilterBar({ filters, onChange, decisions, tools, hasUndecided }: {
  filters: AuditFilters;
  onChange: (next: AuditFilters) => void;
  decisions: string[];
  tools: string[];
  hasUndecided: boolean;
}) {
  const id = useId();
  const set = (patch: Partial<AuditFilters>) => onChange({ ...filters, ...patch });
  return (
    <div className="act-filters" role="search" aria-label={tr("activity.filtri_dell_attivita")}>
      <div className="act-search">
        <Icon name="search" size={15} />
        <input
          type="search" className="act-search-input" aria-label={tr("activity.cerca_nell_attivita")} placeholder={tr("activity.cerca_in_azione_tool_dettaglio")}
          value={filters.q} onChange={(e) => set({ q: e.target.value })}
        />
      </div>
      <Segmented label={tr("activity.filtro_per_attore")} value={filters.actor} onChange={(actor) => set({ actor })} options={ACTORS} className="seg-wrap" />
      <div className="act-selects">
        <div className="field">
          <label htmlFor={`${id}-decision`}>{tr("activity.decisione")}</label>
          <Select id={`${id}-decision`} value={filters.decision} onChange={(e) => set({ decision: e.target.value })}>
            <option value={ALL}>{tr("activity.tutte")}</option>
            {decisions.map((d) => <option key={d} value={d}>{decisionInfo(d).label}</option>)}
            {hasUndecided && <option value="none">{tr("activity.senza_decisione")}</option>}
          </Select>
        </div>
        <div className="field">
          <label htmlFor={`${id}-tool`}>{tr("activity.tool")}</label>
          <Select id={`${id}-tool`} value={filters.tool} onChange={(e) => set({ tool: e.target.value })}>
            <option value={ALL}>{tr("activity.tutti")}</option>
            {tools.map((t) => <option key={t} value={t}>{t}</option>)}
          </Select>
        </div>
        <Button variant="ghost" icon="close" disabled={!isFiltering(filters)} onClick={() => onChange({ actor: ALL, decision: ALL, tool: ALL, q: "" })}>{tr("activity.azzera_i_filtri")}</Button>
      </div>
    </div>
  );
}
