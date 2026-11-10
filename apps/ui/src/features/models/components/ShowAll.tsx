import { setQuery, useQueryParams } from "../../../app/router";
import { useUiMode } from "../../../app/useUiMode";
import { Toggle } from "../../../shared/ui";

/**
 * In simple mode the model lists show only what can run on this device; "Mostra tutti" (kept in
 * the address as `?tutti=1`) adds the rest. In the developer view nothing is hidden by default.
 */
export function useRunnableFilter(): { simple: boolean; showAll: boolean; compatOnly: boolean } {
  const [mode] = useUiMode();
  const query = useQueryParams();
  return { simple: mode === "simple", showAll: query.tutti === "1", compatOnly: query.compat === "1" };
}

export function ShowAllToggle({ showAll, hidden }: { showAll: boolean; hidden: number }) {
  return (
    <div className="field">
      <span className="field-label" id="mh-showall">Mostra tutti</span>
      <span className="row">
        <Toggle checked={showAll} label="Mostra tutti i modelli, anche quelli che non girano su questo dispositivo" onChange={(v) => setQuery({ tutti: v ? "1" : "" })} />
        {!showAll && hidden > 0 && <span className="faint text-sm">{hidden === 1 ? "1 nascosto: non ci sta" : `${hidden} nascosti: non ci stanno`}</span>}
      </span>
    </div>
  );
}
