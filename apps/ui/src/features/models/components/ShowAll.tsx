import { setQuery, useQueryParams } from "../../../app/router";
import { useUiMode } from "../../../app/useUiMode";
import { Toggle } from "../../../shared/ui";

import { t } from "../../../shared/i18n";
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
      <span className="field-label" id="mh-showall">{t("models.mostra_tutti")}</span>
      <span className="row">
        <Toggle checked={showAll} label={t("models.mostra_tutti_i_modelli_anche")} onChange={(v) => setQuery({ tutti: v ? "1" : "" })} />
        {!showAll && hidden > 0 && <span className="faint text-sm">{hidden === 1 ? t("models.1_nascosto_non_ci_sta") : t("models.nascosti_non_ci_stanno", { hidden })}</span>}
      </span>
    </div>
  );
}
