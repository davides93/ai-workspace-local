// Locale-aware formatters built on Intl. They follow the active interface language
// (`ui.language`, see shared/i18n): Italian gives the decimal comma ("1,5 GB") and the it-IT
// date layout, English the English ones. Components re-render on a language change (the app
// remounts below the providers), so every call reads the current tag.
import { intlTag } from "../i18n/locale";
import { t as tr } from "../i18n/t";

/** The BCP 47 tag the formatters use now. */
export const currentLocaleTag = (): string => intlTag();

const numberFormats = new Map<string, Intl.NumberFormat>();
function nf(options: Intl.NumberFormatOptions): Intl.NumberFormat {
  const LOCALE = intlTag();
  const key = `${LOCALE}|${JSON.stringify(options)}`;
  let f = numberFormats.get(key);
  if (!f) {
    f = new Intl.NumberFormat(LOCALE, options);
    numberFormats.set(key, f);
  }
  return f;
}

/** Plain number with at most `digits` decimals ("1,5", "12.345"). */
export function formatNumber(n: number, digits = 1): string {
  return nf({ maximumFractionDigits: digits }).format(n);
}

/** Compact count ("1,5K", "2,3 Mln"). */
export function formatCompact(n: number): string {
  return nf({ notation: "compact", maximumFractionDigits: 1 }).format(n);
}

/** Ratio 0..1 as percentage ("75%"). */
export function formatPercent(ratio: number, digits = 0): string {
  return nf({ style: "percent", maximumFractionDigits: digits }).format(ratio);
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"];

/** Byte count with binary units: "212 KB", "21,2 GB". Zero or invalid values render as an em dash. */
export function formatBytes(n: number | undefined | null): string {
  if (!n || !Number.isFinite(n) || n < 0) return "—";
  let value = n;
  let i = 0;
  while (value >= 1024 && i < BYTE_UNITS.length - 1) {
    value /= 1024;
    i++;
  }
  const digits = value >= 100 || i === 0 ? 0 : 1;
  return `${formatNumber(value, digits)} ${BYTE_UNITS[i]}`;
}

/** Transfer speed in bytes per second ("12,4 MB/s"). */
export function formatSpeed(bps: number | undefined | null): string {
  if (!bps || bps <= 0) return "—";
  return `${formatBytes(bps)}/s`;
}

/** Memory expressed in MB: "512 MB", "24 GB", "1,5 GB". */
export function formatMb(mb: number | undefined | null): string {
  if (mb === undefined || mb === null || !Number.isFinite(mb)) return "—";
  if (mb >= 1024) return `${formatNumber(mb / 1024, mb >= 10240 ? 0 : 1)} GB`;
  return `${formatNumber(mb, 0)} MB`;
}

/** Parameter count in billions: "35B", "0,6B". */
export function formatParams(b: number | undefined | null): string {
  return b ? `${formatNumber(b, b < 10 ? 1 : 0)}B` : "—";
}

/** Context window in tokens: 32768 -> "32K". */
export function formatContext(tokens: number | undefined | null): string {
  return tokens ? `${formatNumber(tokens / 1024, 0)}K` : "—";
}

/** Duration in seconds: "1 h 22 min", "4 min", "35 s". */
export function formatDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.round(totalSeconds));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  const rest = m % 60;
  if (h < 48) return rest ? `${h} h ${rest} min` : `${h} h`;
  return tr("time.days", { n: Math.floor(h / 24) });
}

let relFormat: { locale: string; f: Intl.RelativeTimeFormat } | null = null;
function rtf(): Intl.RelativeTimeFormat {
  const LOCALE = intlTag();
  if (!relFormat || relFormat.locale !== LOCALE) relFormat = { locale: LOCALE, f: new Intl.RelativeTimeFormat(LOCALE, { numeric: "always", style: "short" }) };
  return relFormat.f;
}

/** Relative time in a compact form: "ora", "5 min fa", "3 h fa", "2 gg fa" (English: "now", "5 min. ago"). */
export function relTime(iso: string | number | Date, now: number = Date.now()): string {
  const t = iso instanceof Date ? iso.getTime() : new Date(iso).getTime();
  if (Number.isNaN(t)) return "—";
  const d = (now - t) / 1000;
  if (d < 45) return tr("time.now");
  if (d < 3600) return rtf().format(-Math.floor(d / 60), "minute");
  if (d < 86400) return rtf().format(-Math.floor(d / 3600), "hour");
  if (d < 86400 * 60) return rtf().format(-Math.floor(d / 86400), "day");
  return formatDate(t);
}

/** Short date and time ("02/11/2026, 20:15"). */
export function formatDateTime(iso: string | number | Date): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(d);
}

/** Short date ("02/11/2026"). */
export function formatDate(iso: string | number | Date): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return new Intl.DateTimeFormat(intlTag(), { dateStyle: "short" }).format(d);
}
