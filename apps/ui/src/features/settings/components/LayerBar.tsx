import type { ConfigView } from "../../../shared/api/types";
import { Segmented } from "../../../shared/ui";
import { LAYER_HINT, LAYER_LABEL, WRITABLE, type WritableLayer } from "../model/config";

/** Where the editors below write: global, device or workspace. It stays in view while you scroll the settings. */
export function LayerBar({ view, layer, onLayer }: { view: ConfigView; layer: WritableLayer; onLayer: (l: WritableLayer) => void }) {
  const path = view.layers.find((l) => l.name === layer)?.path;
  const workspaceOpen = !!view.layers.find((l) => l.name === "workspace")?.path;
  const layers = WRITABLE.filter((l) => l !== "workspace" || workspaceOpen);
  return (
    <div className="set-bar" role="region" aria-label="Strato di destinazione">
      <span className="set-bar-label text-sm">Scrivi nello strato</span>
      <Segmented label="Strato in cui scrivere" value={layer} onChange={onLayer} options={layers.map((l) => ({ id: l, label: LAYER_LABEL[l], title: LAYER_HINT[l] }))} />
      <span className="faint text-xs set-bar-path">
        {LAYER_HINT[layer]}
        {path && <> · <span className="mono" title={path}>{path}</span></>}
      </span>
      {!workspaceOpen && <span className="faint text-xs set-bar-path">Nessuna cartella di lavoro aperta: lo strato workspace non è disponibile.</span>}
    </div>
  );
}
