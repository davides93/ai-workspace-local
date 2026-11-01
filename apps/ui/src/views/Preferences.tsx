import { useState } from "react";
import { useApp, type Route } from "../state/app";
import { Icon, useAsync } from "../components/ui";
import type { ConfigView, FieldSchema } from "../api/types";

const LAYERS: ("global" | "device" | "workspace")[] = ["global", "device", "workspace"];
const LAYER_LABEL: Record<string, string> = { defaults: "default", global: "globale", device: "device", workspace: "workspace" };
const MORE: { id: Route; label: string; icon: string }[] = [
  { id: "workflows", label: "Workflow", icon: "flow" },
  { id: "devices", label: "Dispositivi", icon: "device" },
  { id: "tools", label: "Tool e MCP", icon: "tool" },
  { id: "activity", label: "Attività", icon: "activity" },
];

function winner(view: ConfigView, key: string): string {
  let w = "defaults";
  for (const l of view.layers) if (key in l.values) w = l.name;
  return w;
}

export function PreferencesView() {
  const { api, toast, go, theme, setTheme } = useApp();
  const cfg = useAsync(() => api.config(), [api]);
  const [layer, setLayer] = useState<"global" | "device" | "workspace">("global");

  const setValue = async (key: string, value: unknown) => {
    try {
      const v = await api.setConfig(layer, key, value);
      cfg.setData(v);
      toast(`${key} salvato nello strato ${LAYER_LABEL[layer]}`);
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  };

  return (
    <div className="page">
      <nav className="only-phone panel" style={{ marginBottom: 18, flexDirection: "column" }} aria-label="Altre sezioni">
        <div className="list">
          {MORE.map((m) => (
            <div key={m.id} className="list-item clickable" role="button" tabIndex={0} onClick={() => go(m.id)} onKeyDown={(e) => e.key === "Enter" && go(m.id)}>
              <Icon name={m.icon} /> <span className="grow">{m.label}</span>
            </div>
          ))}
        </div>
      </nav>
      <div className="page-head">
        <div className="titles">
          <span className="eyebrow">Controllo</span>
          <h1>Impostazioni</h1>
          <span className="muted">Configurazione a strati: default → globale → device → workspace. Per ogni valore vedi quale strato vince.</span>
        </div>
        <div className="actions">
          <div className="seg" role="group" aria-label="Tema">
            {(["system", "light", "dark"] as const).map((t) => <button key={t} aria-pressed={theme === t} onClick={() => setTheme(t)}>{t === "system" ? "Sistema" : t === "light" ? "Chiaro" : "Scuro"}</button>)}
          </div>
        </div>
      </div>

      <div className="row" style={{ marginBottom: 14 }}>
        <span className="muted" style={{ fontSize: "var(--fs-sm)" }}>Scrivi nello strato:</span>
        <div className="seg" role="group" aria-label="Strato di destinazione">
          {LAYERS.map((l) => <button key={l} aria-pressed={layer === l} onClick={() => setLayer(l)}>{LAYER_LABEL[l]}</button>)}
        </div>
        <span className="faint mono" style={{ fontSize: "var(--fs-xs)" }}>{cfg.data?.layers.find((l) => l.name === layer)?.path}</span>
      </div>

      {cfg.data && (
        <div className="table-wrap">
          <table className="data">
            <thead><tr><th>Impostazione</th><th>Valore effettivo</th><th>Strati</th><th>Modifica ({LAYER_LABEL[layer]})</th></tr></thead>
            <tbody>
              {Object.entries(cfg.data.schema.properties).map(([key, f]) => {
                const w = winner(cfg.data!, key);
                return (
                  <tr key={key}>
                    <td><b>{f.title ?? key}</b><div className="mono faint" style={{ fontSize: "var(--fs-xs)" }}>{key}</div></td>
                    <td className="mono">{JSON.stringify(cfg.data!.effective[key] ?? f.default)}</td>
                    <td>
                      <div className="chips">
                        {cfg.data!.layers.filter((l) => key in l.values).map((l) => (
                          <span key={l.name} className={`layer-badge ${l.name === w ? "win" : ""}`} title={`${LAYER_LABEL[l.name]}: ${JSON.stringify(l.values[key])}`}>{LAYER_LABEL[l.name]}</span>
                        ))}
                      </div>
                    </td>
                    <td style={{ minWidth: 180 }}><ConfigInput id={`cfg-${key}`} field={f} value={cfg.data!.layers.find((l) => l.name === layer)?.values[key] ?? cfg.data!.effective[key]} onCommit={(v) => setValue(key, v)} /></td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function ConfigInput({ id, field, value, onCommit }: { id: string; field: FieldSchema; value: unknown; onCommit: (v: unknown) => void }) {
  const [v, setV] = useState<string>(value === undefined ? "" : typeof value === "string" ? value : JSON.stringify(value));
  if (field.type === "boolean") {
    return <span className="toggle"><input id={id} type="checkbox" aria-label={field.title} checked={!!value} onChange={(e) => onCommit(e.target.checked)} /></span>;
  }
  if (field.enum) {
    return (
      <select id={id} className="input" aria-label={field.title} value={String(value ?? "")} onChange={(e) => onCommit(e.target.value)}>
        {field.enum.map((o) => <option key={String(o)} value={String(o)}>{String(o)}</option>)}
      </select>
    );
  }
  return (
    <input id={id} className="input mono" aria-label={field.title} value={v} onChange={(e) => setV(e.target.value)}
      onBlur={() => onCommit(field.type === "integer" ? parseInt(v, 10) : field.type === "number" ? parseFloat(v) : v)}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />
  );
}
