import { useState } from "react";
import { useTheme } from "../../shared/lib/theme";
import { Chip, Page, PageHeader, QueryState, Segmented, Toggle } from "../../shared/ui";
import type { ConfigView, FieldSchema } from "../../shared/api/types";
import { useConfig, useSetConfig } from "./api";

type Layer = "global" | "device" | "workspace";
const LAYERS: Layer[] = ["global", "device", "workspace"];
const LAYER_LABEL: Record<string, string> = { defaults: "default", global: "globale", device: "device", workspace: "workspace" };

function winner(view: ConfigView, key: string): string {
  let w = "defaults";
  for (const l of view.layers) if (key in l.values) w = l.name;
  return w;
}

export function SettingsView() {
  const cfg = useConfig();
  const setConfig = useSetConfig();
  const [theme, setTheme] = useTheme();
  const [layer, setLayer] = useState<Layer>("global");

  const setValue = (key: string, value: unknown) => setConfig.mutate({ layer, key, value, layerLabel: LAYER_LABEL[layer] });

  return (
    <Page>
      <PageHeader
        eyebrow="Controllo" title="Impostazioni"
        subtitle="Configurazione a strati: default → globale → device → workspace. Per ogni valore vedi quale strato vince."
        actions={<Segmented label="Tema" value={theme} onChange={setTheme} options={[{ id: "system", label: "Sistema" }, { id: "light", label: "Chiaro" }, { id: "dark", label: "Scuro" }]} />}
      />

      <div className="row mb-3">
        <span className="muted text-sm">Scrivi nello strato:</span>
        <Segmented label="Strato di destinazione" value={layer} onChange={setLayer} options={LAYERS.map((l) => ({ id: l, label: LAYER_LABEL[l] }))} />
        <span className="faint mono text-xs">{cfg.data?.layers.find((l) => l.name === layer)?.path}</span>
      </div>

      <QueryState query={cfg}>
        {(view) => (
          <div className="table-wrap">
            <table className="data" aria-label="Impostazioni">
              <thead><tr><th>Impostazione</th><th>Valore effettivo</th><th>Strati</th><th>Modifica ({LAYER_LABEL[layer]})</th></tr></thead>
              <tbody>
                {Object.entries(view.schema.properties).map(([key, f]) => {
                  const w = winner(view, key);
                  return (
                    <tr key={key}>
                      <td><b>{f.title ?? key}</b><div className="mono faint text-xs">{key}</div></td>
                      <td className="mono">{JSON.stringify(view.effective[key] ?? f.default)}</td>
                      <td>
                        <div className="chips">
                          {view.layers.filter((l) => key in l.values).map((l) => (
                            <Chip key={l.name} hot={l.name === w} title={`${LAYER_LABEL[l.name]}: ${JSON.stringify(l.values[key])}`}>{LAYER_LABEL[l.name]}</Chip>
                          ))}
                        </div>
                      </td>
                      <td className="cell-edit">
                        {/* The key resets the input when the target layer changes, so it never shows a stale value. */}
                        <ConfigInput key={`${layer}-${key}`} id={`cfg-${key}`} field={f} value={view.layers.find((l) => l.name === layer)?.values[key] ?? view.effective[key]} onCommit={(v) => setValue(key, v)} />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </QueryState>
    </Page>
  );
}

function ConfigInput({ id, field, value, onCommit }: { id: string; field: FieldSchema; value: unknown; onCommit: (v: unknown) => void }) {
  const initial = value === undefined ? "" : typeof value === "string" ? value : JSON.stringify(value);
  const [v, setV] = useState<string>(initial);
  if (field.type === "boolean") return <Toggle id={id} label={field.title} checked={!!value} onChange={onCommit} />;
  if (field.enum) {
    return (
      <select id={id} className="input" aria-label={field.title} value={String(value ?? "")} onChange={(e) => onCommit(e.target.value)}>
        {field.enum.map((o) => <option key={String(o)} value={String(o)}>{String(o)}</option>)}
      </select>
    );
  }
  // Write on blur only when the value actually changed (tabbing through a field must not save).
  const commit = () => {
    if (v === initial) return;
    onCommit(field.type === "integer" ? parseInt(v, 10) : field.type === "number" ? parseFloat(v) : v);
  };
  return (
    <input id={id} className="input mono" aria-label={field.title} value={v} onChange={(e) => setV(e.target.value)}
      onBlur={commit} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />
  );
}
