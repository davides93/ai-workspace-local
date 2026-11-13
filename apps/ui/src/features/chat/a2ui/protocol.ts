// A2UI v0.9 protocol: surfaces, JSON Pointer data model, expression evaluation.
// Pure functions only (no React) so they can be unit tested.
import { currentLocaleTag } from "../../../shared/lib/format";

/**
 * A2UI v0.9 renderer for the basic catalog (https://a2ui.org/specification/v0_9/catalogs/basic/catalog.json).
 * Surfaces are built from server messages (createSurface → updateComponents → updateDataModel),
 * inputs are two-way bound to the surface data model through JSON Pointers, and Buttons report
 * the v0.9 client `action` message with their context resolved.
 */

export const A2UI_VERSION = "v0.9";
export const A2UI_ACTION_NAME = "a2ui.action";
export const A2UI_ACTION_PREFIX = "[Azione A2UI]";

export type Json = null | boolean | number | string | Json[] | { [k: string]: Json };
export type Obj = Record<string, unknown>;

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
export interface Scope {
  data: Obj;
  /** Base path of a list template item, for relative paths. */
  base: string;
}

export const abs = (path: string, scope: Scope) => (path.startsWith("/") ? path : `${scope.base}/${path}`.replace(/\/+/g, "/"));

export function isBinding(v: unknown): v is { path: string } {
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

export const str = (v: unknown) => (v === undefined || v === null ? "" : typeof v === "string" ? v : typeof v === "number" || typeof v === "boolean" ? String(v) : JSON.stringify(v));

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
      return Number(a("value")).toLocaleString(currentLocaleTag(), { minimumFractionDigits: d, maximumFractionDigits: d, useGrouping: a("grouping") !== false });
    }
    case "formatCurrency":
      try {
        return Number(a("value")).toLocaleString(currentLocaleTag(), { style: "currency", currency: str(a("currency")) || "EUR" });
      } catch {
        return str(a("value"));
      }
    case "formatDate": {
      const d = new Date(str(a("value")));
      return Number.isNaN(d.getTime()) ? str(a("value")) : d.toLocaleString(currentLocaleTag());
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

export function failingChecks(c: A2uiComponent, scope: Scope): string[] {
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

