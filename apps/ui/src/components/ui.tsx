import { useEffect, useState, type ReactNode } from "react";
import type { FieldSchema, RuntimeStatus, SettingsSchema } from "../api/types";

// ---------- Icons (1.6px stroke, 20px grid) ----------
const paths: Record<string, string> = {
  chat: "M4 5h12v8H9l-4 3v-3H4z",
  agent: "M10 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6zM4 17c0-3 2.7-5 6-5s6 2 6 5",
  flow: "M5 4h4v4H5zM11 12h4v4h-4zM7 8v4h4",
  chip: "M6 6h8v8H6zM8 3v3M12 3v3M8 14v3M12 14v3M3 8h3M3 12h3M14 8h3M14 12h3",
  cube: "M10 3l6 3.5v7L10 17l-6-3.5v-7zM4 6.5l6 3.5 6-3.5M10 10v7",
  device: "M3 5h14v9H3zM7 17h6M10 14v3",
  tool: "M13 3a4 4 0 0 0-3.6 5.6L4 14l2 2 5.4-5.4A4 4 0 0 0 17 7l-2.5 2.5-2-2L15 5a4 4 0 0 0-2-2z",
  activity: "M3 10h3l2-5 4 10 2-5h3",
  settings: "M10 7a3 3 0 1 1 0 6 3 3 0 0 1 0-6zM10 2v2M10 16v2M2 10h2M16 10h2M4.3 4.3l1.4 1.4M14.3 14.3l1.4 1.4M4.3 15.7l1.4-1.4M14.3 5.7l1.4-1.4",
  plus: "M10 4v12M4 10h12",
  send: "M4 10l12-6-5 13-2-5z",
  stop: "M6 6h8v8H6z",
  power: "M10 3v7M6.2 5.8a6 6 0 1 0 7.6 0",
  play: "M7 5l8 5-8 5z",
  download: "M10 3v10M6 9l4 4 4-4M4 16h12",
  close: "M5 5l10 10M15 5L5 15",
  check: "M4 10l4 4 8-8",
  more: "M5 10h.01M10 10h.01M15 10h.01",
  panel: "M3 4h14v12H3zM12 4v12",
  list: "M4 6h12M4 10h12M4 14h12",
  shield: "M10 3l6 2v5c0 4-3 6-6 7-3-1-6-3-6-7V5z",
  link: "M8 12l4-4M7 9l-2 2a3 3 0 0 0 4 4l2-2M13 11l2-2a3 3 0 0 0-4-4l-2 2",
  trash: "M4 6h12M8 6V4h4v2M6 6l1 11h6l1-11",
  sparkle: "M10 3l1.6 4.4L16 9l-4.4 1.6L10 15l-1.6-4.4L4 9l4.4-1.6z",
  globe: "M10 3a7 7 0 1 1 0 14 7 7 0 0 1 0-14zM3 10h14M10 3c2 2 2.8 4.4 2.8 7S12 15 10 17c-2-2-2.8-4.4-2.8-7S8 5 10 3z",
  terminal: "M3 4h14v12H3zM6 8l3 2-3 2M10 13h4",
  file: "M5 3h7l3 3v11H5zM12 3v3h3",
  mouse: "M10 3a4 4 0 0 1 4 4v6a4 4 0 0 1-8 0V7a4 4 0 0 1 4-4zM10 3v5",
  refresh: "M15 6a6 6 0 1 0 1 5M15 3v3h-3",
  bolt: "M11 2L4 11h5l-1 7 7-9h-5z",
  branch: "M6 3v14M6 13a4 4 0 0 0 4-4h1a3 3 0 0 0 3-3M14 3a2 2 0 1 1 0 4 2 2 0 0 1 0-4zM6 15a2 2 0 1 1 0 4",
};

export function Icon({ name, size = 18, className }: { name: keyof typeof paths | string; size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className={className}>
      <path d={paths[name] ?? paths.more} />
    </svg>
  );
}

// ---------- Status ----------
export function statusTone(s: RuntimeStatus | string): "ok" | "run" | "warn" | "crit" | "off" {
  switch (s) {
    case "running":
    case "connected":
    case "succeeded":
    case "done":
    case "online":
      return "ok";
    case "starting":
    case "waiting_approval":
    case "waiting":
      return "run";
    case "installable":
    case "stopped":
    case "available":
    case "pending":
    case "disconnected":
    case "offline":
      return "off";
    case "error":
    case "failed":
      return "crit";
    case "unsupported":
    case "cancelled":
    case "skipped":
      return "off";
    default:
      return "off";
  }
}

const statusLabel: Record<string, string> = {
  running: "In esecuzione", starting: "Avvio…", available: "Disponibile", installable: "Da installare", unsupported: "Non supportato",
  stopped: "Fermo", error: "Errore", connected: "Connesso", disconnected: "Disconnesso", succeeded: "Completato", failed: "Fallito",
  waiting_approval: "Attende conferma", cancelled: "Annullato", pending: "In coda", done: "Fatto", skipped: "Saltato", waiting: "In attesa",
  online: "Online", offline: "Offline",
};

export function StatusPill({ status }: { status: string }) {
  const tone = statusTone(status);
  const pillTone = tone === "off" ? "neutral" : tone;
  return (
    <span className={`pill ${pillTone}`}>
      <span className={`led ${tone}`} style={{ width: 7, height: 7, boxShadow: "none" }} />
      {statusLabel[status] ?? status}
    </span>
  );
}

export function Meter({ value, max, tone }: { value: number; max: number; tone?: "ok" | "warn" | "crit" }) {
  const pct = max > 0 ? Math.max(0, Math.min(100, (value / max) * 100)) : 0;
  return (
    <div className={`meter ${tone ?? ""}`} role="meter" aria-valuenow={value} aria-valuemin={0} aria-valuemax={max}>
      <i style={{ width: `${pct}%` }} />
    </div>
  );
}

export function formatBytes(n: number): string {
  if (!n) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n >= 100 || i === 0 ? n.toFixed(0) : n.toFixed(1)} ${u[i]}`;
}

export function formatMb(mb?: number): string {
  if (mb === undefined) return "—";
  return mb >= 1024 ? `${(mb / 1024).toFixed(mb >= 10240 ? 0 : 1)} GB` : `${mb} MB`;
}

export function relTime(iso: string): string {
  const d = (Date.now() - new Date(iso).getTime()) / 1000;
  if (d < 60) return "ora";
  if (d < 3600) return `${Math.floor(d / 60)} min fa`;
  if (d < 86400) return `${Math.floor(d / 3600)} h fa`;
  return `${Math.floor(d / 86400)} g fa`;
}

// ---------- Drawer ----------
export function Drawer({ title, subtitle, onClose, children, footer }: { title: string; subtitle?: string; onClose: () => void; children: ReactNode; footer?: ReactNode }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <>
      <div className="drawer-backdrop" onClick={onClose} />
      <aside className="drawer" role="dialog" aria-label={title}>
        <div className="drawer-head">
          <div className="grow">
            <h2>{title}</h2>
            {subtitle && <div className="muted" style={{ fontSize: "var(--fs-sm)" }}>{subtitle}</div>}
          </div>
          <button className="icon-btn" onClick={onClose} aria-label="Chiudi"><Icon name="close" /></button>
        </div>
        <div className="drawer-body">{children}</div>
        {footer && <div className="drawer-foot">{footer}</div>}
      </aside>
    </>
  );
}

// ---------- Generated settings form (JSON Schema subset) ----------
export function SchemaForm({ schema, value, onChange, showAdvanced, idPrefix }: {
  schema: SettingsSchema;
  value: Record<string, unknown>;
  onChange: (v: Record<string, unknown>) => void;
  showAdvanced?: boolean;
  idPrefix: string;
}) {
  const entries = Object.entries(schema.properties).filter(([, f]) => showAdvanced || !f.advanced);
  if (entries.length === 0) return <p className="muted">Questo componente non ha impostazioni.</p>;
  const set = (k: string, v: unknown) => onChange({ ...value, [k]: v });
  return (
    <div className="form-grid">
      {entries.map(([key, f]) => (
        <SchemaField key={key} id={`${idPrefix}-${key}`} name={key} field={f} value={value[key] ?? f.default} onChange={(v) => set(key, v)} />
      ))}
    </div>
  );
}

function SchemaField({ id, name, field, value, onChange }: { id: string; name: string; field: FieldSchema; value: unknown; onChange: (v: unknown) => void }) {
  const label = field.title ?? name;
  const hint = field.description;
  if (field.type === "boolean") {
    return (
      <div className="row" style={{ justifyContent: "space-between", flexWrap: "nowrap" }}>
        <div className="field">
          <label htmlFor={id}>{label}</label>
          {hint && <span className="hint">{hint}</span>}
        </div>
        <span className="toggle"><input id={id} type="checkbox" checked={!!value} onChange={(e) => onChange(e.target.checked)} /></span>
      </div>
    );
  }
  if (field.enum) {
    return (
      <div className="field">
        <label htmlFor={id}>{label}</label>
        <select id={id} className="input" value={String(value ?? "")} onChange={(e) => onChange(field.type === "string" ? e.target.value : Number(e.target.value))}>
          {field.enum.map((o) => <option key={String(o)} value={String(o)}>{String(o)}</option>)}
        </select>
        {hint && <span className="hint">{hint}</span>}
      </div>
    );
  }
  if (field.type === "integer" || field.type === "number") {
    const hasRange = field.minimum !== undefined && field.maximum !== undefined && field.maximum - field.minimum <= 100000;
    const n = typeof value === "number" ? value : Number(value ?? field.default ?? 0);
    return (
      <div className="field">
        <label htmlFor={id}>{label}</label>
        <div className="range-row">
          {hasRange && (
            <input type="range" aria-label={label} min={field.minimum} max={field.maximum} step={field.step ?? (field.type === "integer" ? 1 : 0.01)} value={n}
              onChange={(e) => onChange(field.type === "integer" ? parseInt(e.target.value, 10) : parseFloat(e.target.value))} />
          )}
          <input id={id} className="input num" style={{ width: hasRange ? 110 : "100%" }} type="number" min={field.minimum} max={field.maximum} step={field.step ?? (field.type === "integer" ? 1 : 0.01)} value={n}
            onChange={(e) => onChange(field.type === "integer" ? parseInt(e.target.value || "0", 10) : parseFloat(e.target.value || "0"))} />
        </div>
        {hint && <span className="hint">{hint}</span>}
      </div>
    );
  }
  if (field.type === "array") {
    const arr = Array.isArray(value) ? (value as string[]) : [];
    return (
      <div className="field">
        <label htmlFor={id}>{label}</label>
        <input id={id} className="input mono" placeholder={field.placeholder ?? "valori separati da spazio"} value={arr.join(" ")}
          onChange={(e) => onChange(e.target.value.split(/\s+/).filter(Boolean))} />
        {hint && <span className="hint">{hint}</span>}
      </div>
    );
  }
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <input id={id} className="input" type={field.secret ? "password" : "text"} placeholder={field.placeholder} value={String(value ?? "")} onChange={(e) => onChange(e.target.value)} />
      {hint && <span className="hint">{hint}</span>}
    </div>
  );
}

// ---------- Minimal, safe Markdown (no HTML passthrough) ----------
export function Markdown({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  let i = 0;
  let key = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("```")) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) buf.push(lines[i++]);
      i++;
      blocks.push(<pre key={key++}><code>{buf.join("\n")}</code></pre>);
      continue;
    }
    if (/^\|.*\|\s*$/.test(line) && i + 1 < lines.length && /^\|[\s:|-]+\|\s*$/.test(lines[i + 1])) {
      const rows: string[][] = [];
      const cells = (l: string) => l.trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
      const head = cells(line);
      i += 2;
      while (i < lines.length && /^\|.*\|\s*$/.test(lines[i])) rows.push(cells(lines[i++]));
      blocks.push(
        <table key={key++}>
          <thead><tr>{head.map((h, j) => <th key={j}>{inline(h)}</th>)}</tr></thead>
          <tbody>{rows.map((r, ri) => <tr key={ri}>{r.map((c, j) => <td key={j}>{inline(c)}</td>)}</tr>)}</tbody>
        </table>,
      );
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\./.test(line);
      const items: string[] = [];
      while (i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*([-*]|\d+\.)\s+/, ""));
      const children = items.map((it, j) => <li key={j}>{inline(it)}</li>);
      blocks.push(ordered ? <ol key={key++}>{children}</ol> : <ul key={key++}>{children}</ul>);
      continue;
    }
    if (/^#{1,3}\s/.test(line)) {
      blocks.push(<h3 key={key++}>{inline(line.replace(/^#{1,3}\s/, ""))}</h3>);
      i++;
      continue;
    }
    if (line.trim() === "") {
      i++;
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() !== "" && !lines[i].startsWith("```") && !/^\|.*\|\s*$/.test(lines[i]) && !/^\s*([-*]|\d+\.)\s+/.test(lines[i])) para.push(lines[i++]);
    blocks.push(<p key={key++}>{inline(para.join(" "))}</p>);
  }
  return <div className="md">{blocks}</div>;
}

function inline(s: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\(https?:\/\/[^)\s]+\))/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(s))) {
    if (m.index > last) out.push(s.slice(last, m.index));
    const t = m[0];
    if (t.startsWith("`")) out.push(<code key={k++}>{t.slice(1, -1)}</code>);
    else if (t.startsWith("**")) out.push(<strong key={k++}>{t.slice(2, -2)}</strong>);
    else {
      const mm = /^\[([^\]]+)\]\((.+)\)$/.exec(t)!;
      out.push(<a key={k++} href={mm[2]} target="_blank" rel="noreferrer">{mm[1]}</a>);
    }
    last = m.index + t.length;
  }
  if (last < s.length) out.push(s.slice(last));
  return out;
}

/** Plain text where `backticks` become <code>. */
export function InlineText({ text }: { text: string }) {
  const parts = text.split(/(`[^`]+`)/g);
  return <>{parts.map((p, i) => (p.startsWith("`") && p.endsWith("`") && p.length > 1 ? <code key={i}>{p.slice(1, -1)}</code> : p))}</>;
}

// ---------- Async data hook ----------
export function useAsync<T>(fn: () => Promise<T>, deps: unknown[], pollMs?: number) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let alive = true;
    fn().then((d) => alive && (setData(d), setError(null))).catch((e) => alive && setError(String(e?.message ?? e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
    return () => { alive = false; };
  }, [...deps, tick]);
  useEffect(() => {
    if (!pollMs) return;
    const t = setInterval(() => setTick((x) => x + 1), pollMs);
    return () => clearInterval(t);
  }, [pollMs]);
  return { data, error, reload: () => setTick((x) => x + 1), setData };
}
