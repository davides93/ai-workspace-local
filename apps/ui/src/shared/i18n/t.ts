import { createElement, Fragment, useCallback, type ReactNode } from "react";
import { getLocale, intlTag, useLocale, type Locale } from "./locale";
import { en } from "./en";
import { it, type MessageKey } from "./it";

export type { MessageKey } from "./it";
export type Params = Record<string, string | number>;

const catalogs: Record<Locale, Record<MessageKey, string>> = { it, en };

/**
 * Message syntax (a small subset of ICU MessageFormat):
 *  - `{name}`: a parameter;
 *  - `{n, plural, one {# file} other {# file}}`: plural by `Intl.PluralRules` of the active
 *    language (`zero`, `one`, `two`, `few`, `many`, `other` and exact `=0` / `=1` branches);
 *    `#` is the number, formatted for the locale.
 */
function skipGroup(s: string, open: number): number {
  let depth = 0;
  for (let i = open; i < s.length; i++) {
    if (s[i] === "{") depth++;
    else if (s[i] === "}" && --depth === 0) return i;
  }
  return s.length - 1;
}

function parsePlural(body: string): Map<string, string> {
  const branches = new Map<string, string>();
  let i = 0;
  while (i < body.length) {
    while (i < body.length && /\s/.test(body[i])) i++;
    const start = i;
    while (i < body.length && !/[\s{]/.test(body[i])) i++;
    const name = body.slice(start, i);
    while (i < body.length && /\s/.test(body[i])) i++;
    if (body[i] !== "{" || !name) break;
    const end = skipGroup(body, i);
    branches.set(name, body.slice(i + 1, end));
    i = end + 1;
  }
  return branches;
}

export function formatMessage(message: string, params: Params | undefined, locale: Locale): string {
  let out = "";
  let i = 0;
  while (i < message.length) {
    const ch = message[i];
    if (ch !== "{") {
      out += ch;
      i++;
      continue;
    }
    const end = skipGroup(message, i);
    const inner = message.slice(i + 1, end);
    i = end + 1;
    const comma = inner.indexOf(",");
    if (comma < 0) {
      const v = params?.[inner.trim()];
      out += v === undefined ? `{${inner}}` : String(v);
      continue;
    }
    const name = inner.slice(0, comma).trim();
    const rest = inner.slice(comma + 1).trim();
    if (!rest.startsWith("plural")) {
      out += `{${inner}}`;
      continue;
    }
    const raw = params?.[name];
    const n = typeof raw === "number" ? raw : Number(raw);
    const branches = parsePlural(rest.slice("plural".length).replace(/^\s*,/, ""));
    const category = new Intl.PluralRules(intlTag(locale)).select(n);
    const branch = branches.get(`=${n}`) ?? branches.get(category) ?? branches.get("other") ?? "";
    const shown = new Intl.NumberFormat(intlTag(locale)).format(n);
    out += formatMessage(branch.replace(/#/g, shown), params, locale);
  }
  return out;
}

/** Translates `key` into the active language. Usable anywhere; React components should prefer `useT()` so they re-render on a language change. */
export function t(key: MessageKey, params?: Params): string {
  const locale = getLocale();
  return formatMessage(catalogs[locale][key] ?? it[key] ?? key, params, locale);
}

export type TFunction = typeof t;

/** `t` bound to the active language; the component re-renders when the language changes. */
export function useT(): TFunction {
  const locale = useLocale();
  return useCallback<TFunction>((key, params) => formatMessage(catalogs[locale][key] ?? it[key] ?? key, params, locale), [locale]);
}

/**
 * A record of translated strings that is safe to declare at module level: every read goes through
 * `t()`, so it follows the active language (a plain object of `t()` results would be frozen in the
 * language active when the module was imported).
 */
export function labelMap<K extends string>(keys: Record<K, MessageKey>): Record<K, string> {
  const out = {} as Record<K, string>;
  for (const k of Object.keys(keys) as K[]) Object.defineProperty(out, k, { enumerable: true, get: () => t(keys[k]) });
  return out;
}

/**
 * Translates a message that contains inline elements (a bold word, a link, `<code>`): each
 * `{slot}` of the message is replaced by the matching node of `slots`, so the translator can move
 * it to wherever the other language needs it.
 */
export function tNode(key: MessageKey, slots: Record<string, ReactNode>, params?: Params): ReactNode {
  const mark = "\u0000";
  const slotParams: Params = {};
  for (const name of Object.keys(slots)) slotParams[name] = `${mark}${name}${mark}`;
  const text = t(key, { ...params, ...slotParams });
  const parts = text.split(mark);
  return createElement(
    Fragment,
    null,
    ...parts.map((part, i) => (i % 2 === 0 ? part : createElement(Fragment, { key: i }, slots[part]))),
  );
}
