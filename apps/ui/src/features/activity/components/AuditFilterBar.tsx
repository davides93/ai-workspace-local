import { useId } from "react";
import { Button, Icon, Segmented, Select } from "../../../shared/ui";
import { ACTOR_LABEL, ALL, decisionInfo, isFiltering, type AuditFilters } from "../model/filter";

const ACTORS = [{ id: ALL, label: "Tutto" }, ...Object.entries(ACTOR_LABEL).map(([id, label]) => ({ id, label: id === "agent" ? "Agenti" : label }))];

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
    <div className="act-filters" role="search" aria-label="Filtri dell'attività">
      <div className="act-search">
        <Icon name="search" size={15} />
        <input
          type="search" className="act-search-input" aria-label="Cerca nell'attività" placeholder="Cerca in azione, tool, dettaglio, run…"
          value={filters.q} onChange={(e) => set({ q: e.target.value })}
        />
      </div>
      <Segmented label="Filtro per attore" value={filters.actor} onChange={(actor) => set({ actor })} options={ACTORS} className="seg-wrap" />
      <div className="act-selects">
        <div className="field">
          <label htmlFor={`${id}-decision`}>Decisione</label>
          <Select id={`${id}-decision`} value={filters.decision} onChange={(e) => set({ decision: e.target.value })}>
            <option value={ALL}>Tutte</option>
            {decisions.map((d) => <option key={d} value={d}>{decisionInfo(d).label}</option>)}
            {hasUndecided && <option value="none">Senza decisione</option>}
          </Select>
        </div>
        <div className="field">
          <label htmlFor={`${id}-tool`}>Tool</label>
          <Select id={`${id}-tool`} value={filters.tool} onChange={(e) => set({ tool: e.target.value })}>
            <option value={ALL}>Tutti</option>
            {tools.map((t) => <option key={t} value={t}>{t}</option>)}
          </Select>
        </div>
        <Button variant="ghost" icon="close" disabled={!isFiltering(filters)} onClick={() => onChange({ actor: ALL, decision: ALL, tool: ALL, q: "" })}>Azzera i filtri</Button>
      </div>
    </div>
  );
}
