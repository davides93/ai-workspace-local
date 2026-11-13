import type { ConfigView } from "../../../shared/api/types";
import { Button, Chip, Icon } from "../../../shared/ui";
import {
  definingLayers, effectiveValue, formatValue, hasOwn, hintOf, LAYER_LABEL, optionLabel, overriddenBy, shortTitle, shownValue, winningLayer,
  type SettingEntry, type WritableLayer,
} from "../model/config";
import { SettingControl } from "./SettingControl";

import { t, t as tr } from "../../../shared/i18n";
/** One setting: what it is, which layer wins, and the editor for the layer you are writing to. */
export function SettingRow({ view, entry, layer, onCommit }: {
  view: ConfigView;
  entry: SettingEntry;
  /** The layer the editors write to. */
  layer: WritableLayer;
  /** `value: null` removes the key from the layer. */
  onCommit: (key: string, title: string, value: unknown) => void;
}) {
  const { key, field } = entry;
  const id = `set-${key.replace(/\./g, "-")}`;
  const title = field.title ?? key;
  const winner = winningLayer(view, key);
  const others = definingLayers(view, key).filter((l) => l !== winner);
  const own = hasOwn(view, layer, key);
  const hiddenBy = overriddenBy(view, layer, key);
  const hint = hintOf(key, field);

  return (
    <li className="set-row">
      <div className="set-info">
        <label htmlFor={id} className="set-title">{title}</label>
        <code className="set-key">{key}</code>
        {hint && <p className="set-hint">{hint}</p>}
        <div className="set-layers">
          <span className="faint text-xs">{t("settings.strato_vincente")}</span>
          <Chip hot={winner !== "defaults"} title={winner === "defaults" ? t("settings.nessuno_strato_definisce_questo_valore") : t("settings.il_valore_effettivo_viene_dallo", { p: LAYER_LABEL[winner] })}>
            {LAYER_LABEL[winner]}
          </Chip>
          {others.length > 0 && <span className="faint text-xs">{t("settings.anche_in")}</span>}
          {others.map((l) => <Chip key={l} title={t("settings.lo_strato_lo_definisce_ma", { p: LAYER_LABEL[l] })}>{LAYER_LABEL[l]}</Chip>)}
        </div>
      </div>
      <div className="set-edit">
        {/* Keyed by layer: a draft typed for one layer never leaks into another. */}
        <SettingControl
          key={layer} id={id} field={field} value={shownValue(view, layer, key, field)}
          onCommit={(v) => onCommit(key, shortTitle(title), v)} optionLabel={(v) => optionLabel(key, v)}
        />
        <div className="set-meta">
          <span className="faint text-xs">{own ? t("settings.valore_nello_strato", { p: LAYER_LABEL[layer] }) : t("settings.eredita_da", { p: LAYER_LABEL[winner] })}</span>
          <Button
            size="sm" variant="ghost" icon="refresh" disabled={!own} aria-label={tr("settings.ripristina_nello_strato", { title, layer: LAYER_LABEL[layer] })}
            title={own ? tr("settings.toglie_il_valore_dallo_strato", { layer: LAYER_LABEL[layer] }) : tr("settings.questo_strato_non_definisce_un")} onClick={() => onCommit(key, shortTitle(title), null)}
          >
            {t("settings.ripristina")}</Button>
        </div>
        {hiddenBy && (
          <p className="set-warn text-xs">
            <Icon name="alert" size={13} />
            <span>
              {own ? t("settings.lo_strato_lo_sovrascrive", { p: LAYER_LABEL[hiddenBy] }) : t("settings.lo_strato_definisce_gia_un", { p: LAYER_LABEL[hiddenBy] })}
              {t("settings.il_valore_effettivo_resta")}<b>{formatValue(effectiveValue(view, key, field))}</b>.
            </span>
          </p>
        )}
      </div>
    </li>
  );
}
