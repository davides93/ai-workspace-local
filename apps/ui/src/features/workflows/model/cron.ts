import { t as tr, tFields } from "../../../shared/i18n";
// 5-field cron as the backend reads it (crates/aiw-workflow/src/schedule.rs): `min hour day month weekday`,
// weekday 0 or 7 = Sunday, 1 = Monday. The backend's `cron` crate requires day-of-month AND weekday to match
// when both are restricted (unlike Vixie cron, which accepts either). The scheduler evaluates the expression in the
// local time of the machine that runs the backend; the preview uses the local time of this browser.

export interface CronFields {
  minute: Set<number>;
  hour: Set<number>;
  day: Set<number>;
  month: Set<number>;
  /** 0 = Sunday … 6 = Saturday */
  weekday: Set<number>;
  /** The field text, to describe the expression. */
  raw: [string, string, string, string, string];
}

export type CronResult = { ok: true; fields: CronFields } | { ok: false; error: string };

const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
const DAYS = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

interface Spec {
  label: string;
  min: number;
  max: number;
  names?: string[];
  /** Names start counting at this number (months at 1, weekdays at 0). */
  nameBase: number;
}

const SPECS: Spec[] = [
  tFields({ min: 0, max: 59, nameBase: 0 }, { label: "workflows.cron_field_minutes" }),
  tFields({ min: 0, max: 23, nameBase: 0 }, { label: "workflows.cron_field_hours" }),
  tFields({ min: 1, max: 31, nameBase: 1 }, { label: "workflows.cron_field_day_of_month" }),
  tFields({ min: 1, max: 12, names: MONTHS, nameBase: 1 }, { label: "workflows.cron_field_month" }),
  tFields({ min: 0, max: 7, names: DAYS, nameBase: 0 }, { label: "workflows.cron_field_day_of_week" }),
];

function value(token: string, spec: Spec): number {
  const t = token.toLowerCase();
  const named = spec.names?.indexOf(t.slice(0, 3));
  if (spec.names && t.length >= 3 && /^[a-z]+$/.test(t) && named !== undefined && named >= 0) return named + spec.nameBase;
  if (!/^\d+$/.test(t)) throw new Error(tr("workflows.non_e_un_valore_valido", { token, label: spec.label }));
  const n = Number(t);
  if (n < spec.min || n > spec.max) throw new Error(tr("workflows.e_fuori_dall_intervallo", { label: spec.label, n, min: spec.min, max: spec.max }));
  return n;
}

function parseField(text: string, spec: Spec): Set<number> {
  const out = new Set<number>();
  for (const part of text.split(",")) {
    if (part === "") throw new Error(tr("workflows.elemento_vuoto", { label: spec.label }));
    const [range, stepText, extra] = part.split("/");
    if (extra !== undefined) throw new Error(tr("workflows.troppi_in", { label: spec.label, part }));
    let step = 1;
    if (stepText !== undefined) {
      if (!/^\d+$/.test(stepText) || Number(stepText) < 1) throw new Error(tr("workflows.il_passo_non_e_valido", { label: spec.label, stepText }));
      step = Number(stepText);
    }
    let lo: number;
    let hi: number;
    if (range === "*" || range === "?") {
      lo = spec.min;
      hi = spec.max;
    } else if (range.includes("-")) {
      const [a, b, more] = range.split("-");
      if (more !== undefined) throw new Error(tr("workflows.intervallo_non_valido", { label: spec.label, range }));
      lo = value(a, spec);
      hi = value(b, spec);
      if (hi < lo) throw new Error(tr("workflows.l_intervallo_e_al_contrario", { label: spec.label, range }));
    } else {
      lo = value(range, spec);
      hi = stepText === undefined ? lo : spec.max;
    }
    for (let v = lo; v <= hi; v += step) out.add(v);
  }
  return out;
}

/** Parses a 5-field expression; the error says which field is wrong. */
export function parseCron(expr: string): CronResult {
  const parts = expr.trim().split(/\s+/).filter(Boolean);
  if (parts.length !== 5) {
    return { ok: false, error: tr("workflows.cron_need_5_fields", { count: parts.length }) };
  }
  try {
    const [minute, hour, day, month, weekday] = parts.map((p, i) => parseField(p, SPECS[i]));
    // 7 is Sunday too.
    if (weekday.has(7)) {
      weekday.delete(7);
      weekday.add(0);
    }
    return { ok: true, fields: { minute, hour, day, month, weekday, raw: parts as CronFields["raw"] } };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : tr("workflows.cron_invalid") };
  }
}

/** First time after `after` (minute precision, local time) at which the expression fires, or null within `maxDays`. */
export function nextCronRun(expr: string, after: Date, maxDays = 366 * 5): Date | null {
  const r = parseCron(expr);
  if (!r.ok) return null;
  const { minute, hour, day, month, weekday } = r.fields;
  const t = new Date(after.getTime());
  t.setSeconds(0, 0);
  t.setMinutes(t.getMinutes() + 1);
  const limit = after.getTime() + maxDays * 86_400_000;
  while (t.getTime() <= limit) {
    if (!month.has(t.getMonth() + 1)) {
      t.setMonth(t.getMonth() + 1, 1);
      t.setHours(0, 0, 0, 0);
      continue;
    }
    if (!day.has(t.getDate()) || !weekday.has(t.getDay())) {
      t.setDate(t.getDate() + 1);
      t.setHours(0, 0, 0, 0);
      continue;
    }
    if (!hour.has(t.getHours())) {
      t.setHours(t.getHours() + 1, 0, 0, 0);
      continue;
    }
    if (!minute.has(t.getMinutes())) {
      t.setMinutes(t.getMinutes() + 1, 0, 0);
      continue;
    }
    return t;
  }
  return null;
}

const dayNames = (): string[] => [tr("workflows.day_sunday"), tr("workflows.day_monday"), tr("workflows.day_tuesday"), tr("workflows.day_wednesday"), tr("workflows.day_thursday"), tr("workflows.day_friday"), tr("workflows.day_saturday")];
const pad = (n: number) => String(n).padStart(2, "0");

/** Plain-language reading of the common shapes (`0 8 * * 1-5` → "Giorni feriali alle 08:00"); null for anything else. */
export function describeCron(expr: string): string | null {
  const r = parseCron(expr);
  if (!r.ok) return null;
  const [m, h, d, mo, w] = r.fields.raw;
  const fixed = (s: string) => /^\d+$/.test(s);
  if (m.startsWith("*/") && h === "*" && d === "*" && mo === "*" && w === "*" && /^\d+$/.test(m.slice(2))) return tr("workflows.cron_every_minutes", { n: m.slice(2) });
  if (m === "0" && h.startsWith("*/") && d === "*" && mo === "*" && w === "*" && /^\d+$/.test(h.slice(2))) return tr("workflows.cron_every_hours", { n: h.slice(2) });
  if (!fixed(m) || !fixed(h) || mo !== "*") return null;
  const at = tr("workflows.cron_at", { hour: pad(Number(h)), minute: pad(Number(m)) });
  const DAY_NAMES = dayNames();
  if (d === "*" && w === "*") return tr("workflows.cron_every_day", { at });
  if (d === "*") {
    const days = [...r.fields.weekday].sort((a, b) => a - b);
    if (days.join() === "1,2,3,4,5") return tr("workflows.cron_weekdays", { at });
    if (days.join() === "0,6") return tr("workflows.cron_weekend", { at });
    if (days.length === 1) return tr("workflows.cron_every_weekday", { day: DAY_NAMES[days[0]], at });
    return tr("workflows.cron_days_list", { days: days.map((x) => DAY_NAMES[x].slice(0, 3)).join(", "), at });
  }
  if (w === "*" && fixed(d)) return tr("workflows.cron_day_of_month", { day: Number(d), at });
  return null;
}
