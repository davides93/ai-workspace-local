import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Icon, IconButton, Markdown, ModalShell } from "../../../shared/ui";
import {
  abs, failingChecks, getPath, isBinding, resolve, setPath, str,
  type A2uiAction, type A2uiComponent, type Json, type Obj, type Scope, type SurfaceState,
} from "./protocol";

import { t as tr } from "../../../shared/i18n";
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
  if (surface.deleted) return <div className="a2ui-surface faint">{tr("chat.interfaccia_chiusa")}</div>;
  if (!surface.components.root) return <div className="a2ui-surface faint">{tr("chat.interfaccia_senza_componente_root")}</div>;
  return (
    <div className="a2ui-surface" data-surface={surface.surfaceId} aria-label={tr("chat.interfaccia", { surfaceId: surface.surfaceId })}>
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
      return <span className="a2ui-missing">{tr("chat.componente_non_supportato", { component: c.component })}</span>;
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
        <ModalShell variant="dialog" title={tr("chat.finestra_dell_interfaccia")} onClose={() => setOpen(false)}>
          <div className="overlay-head">
            <span className="grow" />
            <IconButton aria-label={tr("chat.chiudi")} icon="close" onClick={() => setOpen(false)} />
          </div>
          <div className="overlay-body">
            <Node id={String(c.content)} ctx={ctx} scope={scope} />
          </div>
        </ModalShell>
      )}
    </>
  );
}
