import type { ConfigView } from "../../../shared/api/types";
import { Button, Chip, Icon } from "../../../shared/ui";
import {
  definingLayers, effectiveValue, formatValue, hasOwn, hintOf, LAYER_LABEL, optionLabel, overriddenBy, shortTitle, shownValue, winningLayer,
  type SettingEntry, type WritableLayer,
} from "../model/config";
import { SettingControl } from "./SettingControl";

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
          <span className="faint text-xs">Strato vincente</span>
          <Chip hot={winner !== "defaults"} title={winner === "defaults" ? "Nessuno strato definisce questo valore: vale il default" : `Il valore effettivo viene dallo strato ${LAYER_LABEL[winner]}`}>
            {LAYER_LABEL[winner]}
          </Chip>
          {others.length > 0 && <span className="faint text-xs">anche in</span>}
          {others.map((l) => <Chip key={l} title={`Lo strato ${LAYER_LABEL[l]} lo definisce ma è sovrascritto`}>{LAYER_LABEL[l]}</Chip>)}
        </div>
      </div>
      <div className="set-edit">
        {/* Keyed by layer: a draft typed for one layer never leaks into another. */}
        <SettingControl
          key={layer} id={id} field={field} value={shownValue(view, layer, key, field)}
          onCommit={(v) => onCommit(key, shortTitle(title), v)} optionLabel={(v) => optionLabel(key, v)}
        />
        <div className="set-meta">
          <span className="faint text-xs">{own ? `Valore nello strato ${LAYER_LABEL[layer]}` : `Eredita da ${LAYER_LABEL[winner]}`}</span>
          <Button
            size="sm" variant="ghost" icon="refresh" disabled={!own} aria-label={`Ripristina ${title} nello strato ${LAYER_LABEL[layer]}`}
            title={own ? `Toglie il valore dallo strato ${LAYER_LABEL[layer]}` : "Questo strato non definisce un valore"} onClick={() => onCommit(key, shortTitle(title), null)}
          >
            Ripristina
          </Button>
        </div>
        {hiddenBy && (
          <p className="set-warn text-xs">
            <Icon name="alert" size={13} />
            <span>
              {own ? `Lo strato «${LAYER_LABEL[hiddenBy]}» lo sovrascrive` : `Lo strato «${LAYER_LABEL[hiddenBy]}» definisce già un valore: scrivendo qui non cambia nulla`}
              {": il valore effettivo resta "}<b>{formatValue(effectiveValue(view, key, field))}</b>.
            </span>
          </p>
        )}
      </div>
    </li>
  );
}
