import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Icon, Markdown } from "./ui";

/**
 * A2UI v0.9 renderer for the basic catalog (https://a2ui.org/specification/v0_9/catalogs/basic/catalog.json).
 * Surfaces are built from server messages (createSurface → updateComponents → updateDataModel),
 * inputs are two-way bound to the surface data model through JSON Pointers, and Buttons report
 * the v0.9 client `action` message with their context resolved.
 */

export const A2UI_VERSION = "v0.9";
export const A2UI_ACTION_NAME = "a2ui.action";
export const A2UI_ACTION_PREFIX = "[Azione A2UI]";

type Json = null | boolean | number | string | Json[] | { [k: string]: Json };
type Obj = Record<string, unknown>;

export interface A2uiComponent {
  id: string;
  component: string;
  [prop: string]: unknown;
}

export interface A2uiMessage {
  version?: string;
  createSurface?: { surfaceId: string; catalogId?: string };
  updateComponents?: { surfaceId: string; components: A2uiComponent[] };
  updateDataModel?: { surfaceId: string; path?: string; value?: unknown };
  deleteSurface?: { surfaceId: string };
}

export interface SurfaceState {
  surfaceId: string;
  components: Record<string, A2uiComponent>;
  data: Obj;
  deleted?: boolean;
  /** Bumped when the server sends a new data model, so the rendered copy is reset. */
  rev: number;
}

export interface A2uiAction {
  name: string;
  surfaceId: string;
  sourceComponentId: string;
  timestamp: string;
  context: Obj;
}

// ------------------------------------------------------------------ JSON Pointer
const unescape = (t: string) => t.replace(/~1/g, "/").replace(/~0/g, "~");
const tokens = (ptr: string) => (ptr === "" || ptr === "/" ? [] : ptr.replace(/^\//, "").split("/").map(unescape));

export function getPath(data: unknown, ptr: string): unknown {
  let cur: unknown = data;
  for (const t of tokens(ptr)) {
    if (cur === null || typeof cur !== "object") return undefined;
    cur = Array.isArray(cur) ? cur[Number(t)] : (cur as Obj)[t];
  }
  return cur;
}

export function setPath(data: Obj, ptr: string, value: unknown): Obj {
  const ts = tokens(ptr);
  if (ts.length === 0) return (value && typeof value === "object" && !Array.isArray(value) ? value : {}) as Obj;
  const write = (node: unknown, i: number): unknown => {
    const key = ts[i];
    const base: Obj | unknown[] = Array.isArray(node) ? [...node] : node && typeof node === "object" ? { ...(node as Obj) } : /^\d+$/.test(key) ? [] : {};
    const next = i === ts.length - 1 ? value : write(Array.isArray(base) ? base[Number(key)] : (base as Obj)[key], i + 1);
    if (Array.isArray(base)) {
      if (next === undefined) base.splice(Number(key), 1);
      else base[Number(key)] = next;
    } else if (next === undefined) delete (base as Obj)[key];
    else (base as Obj)[key] = next;
    return base;
  };
  return write(data, 0) as Obj;
}

// ------------------------------------------------------------------ messages
/** Nested child components (a common model habit) lifted into the flat list, as the server does. */
function flatten(list: unknown[]): Obj[] {
  const out: Obj[] = [];
  let n = 0;
  const lift = (c: Obj) => {
    const nested: Obj[] = [];
    const take = (v: unknown): unknown => {
      if (v && typeof v === "object" && !Array.isArray(v) && ("component" in (v as Obj) || "type" in (v as Obj))) {
        const o = { ...(v as Obj) };
        if (typeof o.id !== "string" || !o.id) o.id = `c${++n}`;
        nested.push(o);
        return o.id;
      }
      return v;
    };
    const c2: Obj = { ...c };
    for (const k of ["child", "trigger", "content"]) if (k in c2) c2[k] = take(c2[k]);
    if (Array.isArray(c2.children)) c2.children = c2.children.map(take);
    if (Array.isArray(c2.tabs)) c2.tabs = (c2.tabs as Obj[]).map((t) => (t && typeof t === "object" ? { ...t, child: take(t.child) } : t));
    out.push(c2);
    nested.forEach(lift);
  };
  for (const c of list) if (c && typeof c === "object") lift(c as Obj);
  return out;
}

/** Same shortcuts the server normalises (render_ui arguments rebuilt from history). */
export function normaliseComponents(list: unknown[]): A2uiComponent[] {
  const out: A2uiComponent[] = [];
  for (const raw of flatten(list)) {
    if (!raw || typeof raw !== "object") continue;
    const c = { ...(raw as Obj) } as A2uiComponent;
    if (!c.component && typeof c.type === "string") c.component = c.type as string;
    if (c.component === "Button") {
      if (!c.child && (c.label !== undefined || c.text !== undefined)) {
        const child = `${c.id}__label`;
        out.push({ id: child, component: "Text", text: c.label ?? c.text });
        c.child = child;
      }
      if (typeof c.action === "string") c.action = { event: { name: c.action } };
      if (!c.action) c.action = { event: { name: c.id } };
    }
    if (c.component === "Text" && c.text === undefined) c.text = c.content ?? c.label;
    out.push(c);
  }
  return out;
}

/** Messages equivalent to a `render_ui` call (used to rebuild surfaces from the chat history). */
export function renderUiToMessages(args: Obj): A2uiMessage[] {
  const surfaceId = typeof args.surfaceId === "string" && args.surfaceId ? args.surfaceId : "ui";
  const msgs: A2uiMessage[] = [
    { version: A2UI_VERSION, createSurface: { surfaceId } },
    { version: A2UI_VERSION, updateComponents: { surfaceId, components: normaliseComponents(Array.isArray(args.components) ? args.components : []) } },
  ];
  if (args.data && typeof args.data === "object") msgs.push({ version: A2UI_VERSION, updateDataModel: { surfaceId, path: "/", value: args.data } });
  return msgs;
}

export function applyA2ui(prev: SurfaceState | null, msgs: A2uiMessage[]): SurfaceState | null {
  let s = prev;
  for (const m of msgs) {
    if (m.createSurface) s = { surfaceId: m.createSurface.surfaceId, components: s?.components ?? {}, data: s?.data ?? {}, rev: (s?.rev ?? 0) + 1 };
    if (m.updateComponents && s) {
      const components = { ...s.components };
      for (const c of m.updateComponents.components) components[c.id] = c;
      s = { ...s, components };
    }
    if (m.updateDataModel && s) {
      const { path = "/", value } = m.updateDataModel;
      s = { ...s, data: setPath(s.data, path, value), rev: s.rev + 1 };
    }
    if (m.deleteSurface && s) s = { ...s, deleted: true };
  }
  return s;
}

// ------------------------------------------------------------------ evaluation
interface Scope {
  data: Obj;
  /** Base path of a list template item, for relative paths. */
  base: string;
}

const abs = (path: string, scope: Scope) => (path.startsWith("/") ? path : `${scope.base}/${path}`.replace(/\/+/g, "/"));

function isBinding(v: unknown): v is { path: string } {
  return !!v && typeof v === "object" && !Array.isArray(v) && typeof (v as Obj).path === "string" && Object.keys(v as Obj).length === 1;
}
function isCall(v: unknown): v is { call: string; args?: Obj } {
  return !!v && typeof v === "object" && !Array.isArray(v) && typeof (v as Obj).call === "string";
}

export function resolve(v: unknown, scope: Scope): unknown {
  if (isBinding(v)) return getPath(scope.data, abs(v.path, scope));
  if (isCall(v)) return callFn(v.call, v.args ?? {}, scope);
  return v;
}

const str = (v: unknown) => (v === undefined || v === null ? "" : typeof v === "string" ? v : typeof v === "number" || typeof v === "boolean" ? String(v) : JSON.stringify(v));

function callFn(name: string, rawArgs: Obj, scope: Scope): unknown {
  const a = (k: string) => resolve(rawArgs[k], scope);
  switch (name) {
    case "required": {
      const v = a("value");
      return !(v === undefined || v === null || v === "" || (Array.isArray(v) && v.length === 0));
    }
    case "regex":
      try {
        return new RegExp(str(a("pattern"))).test(str(a("value")));
      } catch {
        return false;
      }
    case "length": {
      const n = str(a("value")).length;
      const min = a("min") as number | undefined;
      const max = a("max") as number | undefined;
      return (min === undefined || n >= min) && (max === undefined || n <= max);
    }
    case "numeric": {
      const n = Number(a("value"));
      const min = a("min") as number | undefined;
      const max = a("max") as number | undefined;
      return str(a("value")) !== "" && !Number.isNaN(n) && (min === undefined || n >= min) && (max === undefined || n <= max);
    }
    case "email":
      return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(str(a("value")));
    case "and":
      return ((rawArgs.values as unknown[]) ?? []).every((x) => !!resolve(x, scope));
    case "or":
      return ((rawArgs.values as unknown[]) ?? []).some((x) => !!resolve(x, scope));
    case "not":
      return !a("value");
    case "formatString":
      return str(a("value")).replace(/\$\{([^}]+)\}/g, (_, p: string) => str(getPath(scope.data, abs(p.trim(), scope))));
    case "formatNumber": {
      const d = (a("decimals") as number | undefined) ?? undefined;
      return Number(a("value")).toLocaleString(undefined, { minimumFractionDigits: d, maximumFractionDigits: d, useGrouping: a("grouping") !== false });
    }
    case "formatCurrency":
      try {
        return Number(a("value")).toLocaleString(undefined, { style: "currency", currency: str(a("currency")) || "EUR" });
      } catch {
        return str(a("value"));
      }
    case "formatDate": {
      const d = new Date(str(a("value")));
      return Number.isNaN(d.getTime()) ? str(a("value")) : d.toLocaleString();
    }
    case "pluralize": {
      const n = Number(a("value"));
      const pick = (k: string) => (rawArgs[k] !== undefined ? str(a(k)) : undefined);
      return (n === 0 ? pick("zero") : n === 1 ? pick("one") : n === 2 ? pick("two") : undefined) ?? pick("other") ?? "";
    }
    default:
      return undefined;
  }
}

function failingChecks(c: A2uiComponent, scope: Scope): string[] {
  const checks = Array.isArray(c.checks) ? (c.checks as { condition: unknown; message: string }[]) : [];
  const out = checks.filter((k) => !resolve(k.condition, scope)).map((k) => k.message);
  if (typeof c.validationRegexp === "string") {
    try {
      const v = str(resolve(c.value, scope));
      if (v && !new RegExp(c.validationRegexp).test(v)) out.push("Formato non valido");
    } catch {
      /* invalid regexp from the model: ignore */
    }
  }
  return out;
}

// ------------------------------------------------------------------ rendering
const FLEX: Record<string, string> = {
  start: "flex-start", end: "flex-end", center: "center", stretch: "stretch",
  spaceBetween: "space-between", spaceAround: "space-around", spaceEvenly: "space-evenly",
};
const ICONS: Record<string, string> = {
  add: "plus", check: "check", close: "close", delete: "trash", download: "download", refresh: "refresh", send: "send",
  settings: "settings", play: "play", stop: "stop", folder: "file", attachFile: "file", home: "device", info: "sparkle",
  warning: "shield", lock: "shield", lockOpen: "shield", search: "globe", share: "link", person: "agent", accountCircle: "agent",
  edit: "file", upload: "download", event: "flow", calendarToday: "flow", mail: "send", phone: "device", star: "sparkle",
};

interface Ctx {
  surface: SurfaceState;
  data: Obj;
  setData: (fn: (d: Obj) => Obj) => void;
  onAction: (a: A2uiAction) => void;
  disabled: boolean;
  touched: Set<string>;
  touch: (id: string) => void;
  depth: number;
}

export function A2uiSurface({ surface, onAction, disabled = false }: { surface: SurfaceState; onAction: (a: A2uiAction) => void; disabled?: boolean }) {
  const [data, setDataState] = useState<Obj>(surface.data);
  const [touched, setTouched] = useState<Set<string>>(new Set());
  // A new data model from the agent replaces what the user typed.
  useEffect(() => setDataState(surface.data), [surface.rev, surface.data]);
  const ctx: Ctx = useMemo(
    () => ({
      surface, data, setData: (fn) => setDataState((d) => fn(d)), onAction, disabled, touched,
      touch: (id) => setTouched((t) => (t.has(id) ? t : new Set(t).add(id))), depth: 0,
    }),
    [surface, data, onAction, disabled, touched],
  );
  if (surface.deleted) return <div className="a2ui-surface faint">Interfaccia chiusa.</div>;
  if (!surface.components.root) return <div className="a2ui-surface faint">Interfaccia senza componente root.</div>;
  return (
    <div className="a2ui-surface" data-surface={surface.surfaceId} aria-label={`Interfaccia ${surface.surfaceId}`}>
      <Node id="root" ctx={ctx} scope={{ data, base: "" }} />
    </div>
  );
}

function Node({ id, ctx, scope }: { id: string; ctx: Ctx; scope: Scope }): ReactNode {
  const c = ctx.surface.components[id];
  if (!c) return <span className="a2ui-missing">[{id}?]</span>;
  if (ctx.depth > 40) return null; // cycles in model-written trees
  const inner: Ctx = { ...ctx, depth: ctx.depth + 1 };
  const r = (v: unknown) => resolve(v, scope);
  const bind = (prop: string) => {
    const b = c[prop];
    return isBinding(b) ? abs(b.path, scope) : null;
  };
  const update = (prop: string, value: unknown) => {
    const p = bind(prop);
    if (p) ctx.setData((d) => setPath(d, p, value));
    ctx.touch(c.id);
  };
  const children = (): ReactNode[] => {
    const ch = c.children;
    if (Array.isArray(ch)) return ch.map((k) => <Node key={String(k)} id={String(k)} ctx={inner} scope={scope} />);
    if (ch && typeof ch === "object") {
      const t = ch as { componentId: string; path: string };
      const listPath = abs(t.path, scope);
      const items = getPath(scope.data, listPath);
      return Array.isArray(items)
        ? items.map((_, i) => <Node key={`${t.componentId}-${i}`} id={t.componentId} ctx={inner} scope={{ data: scope.data, base: `${listPath}/${i}` }} />)
        : [];
    }
    return [];
  };
  const errors = ctx.touched.has(c.id) ? failingChecks(c, scope) : [];
  const label = c.label !== undefined ? str(r(c.label)) : "";
  const fieldId = `a2ui-${ctx.surface.surfaceId}-${c.id}${scope.base.replace(/\//g, "-")}`;

  switch (c.component) {
    case "Text": {
      const text = str(r(c.text));
      const v = String(c.variant ?? "body");
      if (/^h[1-5]$/.test(v)) {
        const H = (`h${Math.min(6, Number(v[1]) + 1)}`) as "h2";
        return <H className={`a2ui-text a2ui-${v}`}>{text}</H>;
      }
      if (v === "caption") return <span className="a2ui-caption">{text}</span>;
      return <div className="a2ui-body"><Markdown text={text} /></div>;
    }
    case "Image": {
      const v = String(c.variant ?? "mediumFeature");
      return <img className={`a2ui-img a2ui-img-${v}`} src={str(r(c.url))} alt={str(r(c.description))} style={{ objectFit: (c.fit === "scaleDown" ? "scale-down" : (c.fit as "cover") ?? "cover") }} />;
    }
    case "Icon": {
      const n = r(c.name);
      if (n && typeof n === "object" && typeof (n as Obj).svgPath === "string") {
        return <svg className="a2ui-icon" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true"><path d={(n as Obj).svgPath as string} fill="currentColor" /></svg>;
      }
      return <span className="a2ui-icon" title={str(n)}><Icon name={ICONS[str(n)] ?? "sparkle"} size={18} /></span>;
    }
    case "Video":
      return <video className="a2ui-media" src={str(r(c.url))} controls />;
    case "AudioPlayer":
      return <audio className="a2ui-media" src={str(r(c.url))} controls aria-label={str(r(c.description))} />;
    case "Row":
    case "Column":
    case "List": {
      const dir = c.component === "Row" || c.direction === "horizontal" ? "row" : "column";
      return (
        <div className={`a2ui-flex a2ui-${c.component.toLowerCase()}`}
          style={{ flexDirection: dir, justifyContent: FLEX[String(c.justify ?? "start")], alignItems: FLEX[String(c.align ?? (dir === "row" ? "center" : "stretch"))] }}>
          {children()}
        </div>
      );
    }
    case "Card":
      return <div className="a2ui-card"><Node id={String(c.child)} ctx={inner} scope={scope} /></div>;
    case "Divider":
      return c.axis === "vertical" ? <span className="a2ui-vdivider" role="separator" /> : <hr className="a2ui-divider" />;
    case "Tabs":
      return <Tabs c={c} ctx={inner} scope={scope} />;
    case "Modal":
      return <Modal c={c} ctx={inner} scope={scope} />;
    case "Button": {
      const blocked = failingChecks(c, scope).length > 0;
      const variant = String(c.variant ?? "default");
      const fire = () => {
        const action = c.action as { event?: { name: string; context?: Obj }; functionCall?: { call: string; args?: Obj } } | undefined;
        if (action?.functionCall?.call === "openUrl") {
          const url = str(resolve(action.functionCall.args?.url, scope));
          if (/^https?:\/\//.test(url)) window.open(url, "_blank", "noopener,noreferrer");
          return;
        }
        const ev = action?.event ?? { name: c.id };
        const context: Obj = {};
        for (const [k, v] of Object.entries(ev.context ?? {})) context[k] = resolve(v, scope) as Json;
        ctx.onAction({ name: ev.name, surfaceId: ctx.surface.surfaceId, sourceComponentId: c.id, timestamp: new Date().toISOString(), context });
      };
      return (
        <button type="button" className={`btn ${variant === "primary" ? "primary" : variant === "borderless" ? "ghost" : ""}`} disabled={ctx.disabled || blocked} onClick={fire}>
          <Node id={String(c.child)} ctx={{ ...inner, depth: inner.depth + 1 }} scope={scope} />
        </button>
      );
    }
    case "TextField": {
      const v = String(c.variant ?? "shortText");
      const value = str(r(c.value));
      const common = {
        id: fieldId, className: "input", value, disabled: ctx.disabled,
        onChange: (e: { target: { value: string } }) => update("value", e.target.value),
        onBlur: () => ctx.touch(c.id),
        "aria-invalid": errors.length > 0,
      };
      return (
        <div className="field a2ui-field">
          <label htmlFor={fieldId}>{label}</label>
          {v === "longText" ? <textarea {...common} rows={3} /> : <input {...common} type={v === "number" ? "number" : v === "obscured" ? "password" : "text"} />}
          {errors.map((e) => <span key={e} className="a2ui-error">{e}</span>)}
        </div>
      );
    }
    case "CheckBox":
      return (
        <label className="a2ui-check">
          <input type="checkbox" checked={!!r(c.value)} disabled={ctx.disabled} onChange={(e) => update("value", e.target.checked)} />
          <span>{label}</span>
          {errors.map((e) => <span key={e} className="a2ui-error">{e}</span>)}
        </label>
      );
    case "ChoicePicker": {
      const multi = c.variant === "multipleSelection";
      const chips = c.displayStyle === "chips";
      const raw = r(c.value);
      const selected = Array.isArray(raw) ? raw.map(str) : raw === undefined || raw === null || raw === "" ? [] : [str(raw)];
      const options = (Array.isArray(c.options) ? c.options : []) as { label: unknown; value: string }[];
      const toggle = (val: string) => {
        const next = multi ? (selected.includes(val) ? selected.filter((x) => x !== val) : [...selected, val]) : [val];
        update("value", next);
      };
      return (
        <fieldset className="a2ui-choice" disabled={ctx.disabled}>
          {label && <legend>{label}</legend>}
          <div className={chips ? "a2ui-chips" : "a2ui-options"}>
            {options.map((o) =>
              chips ? (
                <button key={o.value} type="button" className={`pill ${selected.includes(o.value) ? "run" : "neutral"} pill-btn`} aria-pressed={selected.includes(o.value)} onClick={() => toggle(o.value)}>
                  {str(resolve(o.label, scope))}
                </button>
              ) : (
                <label key={o.value} className="a2ui-check">
                  <input type={multi ? "checkbox" : "radio"} name={fieldId} checked={selected.includes(o.value)} onChange={() => toggle(o.value)} />
                  <span>{str(resolve(o.label, scope))}</span>
                </label>
              ),
            )}
          </div>
          {errors.map((e) => <span key={e} className="a2ui-error">{e}</span>)}
        </fieldset>
      );
    }
    case "Slider": {
      const min = Number(c.min ?? 0);
      const max = Number(c.max ?? 100);
      const value = Number(r(c.value) ?? min);
      return (
        <div className="field a2ui-field">
          <label htmlFor={fieldId}>{label} <span className="mono faint">{value}</span></label>
          <input id={fieldId} type="range" min={min} max={max} value={value} disabled={ctx.disabled} onChange={(e) => update("value", Number(e.target.value))} />
        </div>
      );
    }
    case "DateTimeInput": {
      const date = c.enableDate !== false;
      const time = c.enableTime === true;
      const type = date && time ? "datetime-local" : time ? "time" : "date";
      return (
        <div className="field a2ui-field">
          <label htmlFor={fieldId}>{label}</label>
          <input id={fieldId} className="input" type={type} value={str(r(c.value))} min={str(r(c.min)) || undefined} max={str(r(c.max)) || undefined}
            disabled={ctx.disabled} onChange={(e) => update("value", e.target.value)} />
          {errors.map((e) => <span key={e} className="a2ui-error">{e}</span>)}
        </div>
      );
    }
    default:
      return <span className="a2ui-missing">Componente non supportato: {c.component}</span>;
  }
}

function Tabs({ c, ctx, scope }: { c: A2uiComponent; ctx: Ctx; scope: Scope }) {
  const tabs = (Array.isArray(c.tabs) ? c.tabs : []) as { title: unknown; child: string }[];
  const [i, setI] = useState(0);
  const cur = tabs[Math.min(i, tabs.length - 1)];
  return (
    <div className="a2ui-tabs">
      <div className="a2ui-tablist" role="tablist">
        {tabs.map((t, k) => (
          <button key={k} type="button" role="tab" aria-selected={k === i} className={`a2ui-tab ${k === i ? "on" : ""}`} onClick={() => setI(k)}>
            {str(resolve(t.title, scope))}
          </button>
        ))}
      </div>
      {cur && <div role="tabpanel"><Node id={cur.child} ctx={ctx} scope={scope} /></div>}
    </div>
  );
}

function Modal({ c, ctx, scope }: { c: A2uiComponent; ctx: Ctx; scope: Scope }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <span className="a2ui-modal-trigger" onClickCapture={(e) => { e.stopPropagation(); e.preventDefault(); setOpen(true); }}>
        <Node id={String(c.trigger)} ctx={{ ...ctx, onAction: () => setOpen(true) }} scope={scope} />
      </span>
      {open && (
        <div className="a2ui-modal-backdrop" onClick={() => setOpen(false)}>
          <div className="a2ui-modal" role="dialog" aria-modal="true" onClick={(e) => e.stopPropagation()}>
            <button type="button" className="icon-btn a2ui-modal-close" aria-label="Chiudi" onClick={() => setOpen(false)}><Icon name="close" /></button>
            <Node id={String(c.content)} ctx={ctx} scope={scope} />
          </div>
        </div>
      )}
    </>
  );
}

/** Compact description of an action turn for the chat history. */
export function describeAction(text: string): { name: string; context: Obj } | null {
  if (!text.startsWith(A2UI_ACTION_PREFIX)) return null;
  try {
    const v = JSON.parse(text.slice(A2UI_ACTION_PREFIX.length).trim()) as { action?: A2uiAction };
    return v.action ? { name: v.action.name, context: v.action.context ?? {} } : null;
  } catch {
    return null;
  }
}

export const actionText = (a: A2uiAction) => `${A2UI_ACTION_PREFIX} ${JSON.stringify({ version: A2UI_VERSION, action: a })}`;
