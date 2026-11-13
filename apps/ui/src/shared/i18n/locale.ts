import { useSyncExternalStore } from "react";

/** Languages the interface is translated into. */
export type Locale = "it" | "en";
/** Value of the setting `ui.language`: `auto` follows the browser / operating system. */
export type LanguageSetting = "auto" | Locale;

export const LOCALES: Locale[] = ["it", "en"];
export const LANGUAGE_SETTINGS: LanguageSetting[] = ["auto", "it", "en"];
export const DEFAULT_LANGUAGE: LanguageSetting = "auto";
/** Endonyms: each language is always shown in itself, whatever the active one is. */
export const LANGUAGE_NAME: Record<Locale, string> = { it: "Italiano", en: "English" };

export const isLocale = (v: unknown): v is Locale => v === "it" || v === "en";
export const isLanguageSetting = (v: unknown): v is LanguageSetting => v === "auto" || isLocale(v);

/** Italian when the browser / OS language starts with `it`, English for everything else. */
export function localeFromBrowser(languages?: readonly string[] | string | null): Locale {
  const first = typeof languages === "string" ? languages : languages?.[0];
  return (first ?? "").toLowerCase().startsWith("it") ? "it" : "en";
}

export function resolveLocale(setting: LanguageSetting, languages?: readonly string[] | string | null): Locale {
  return setting === "auto" ? localeFromBrowser(languages) : setting;
}

const KEY = "aiw.ui.language";
const listeners = new Set<() => void>();

function browserLanguages(): readonly string[] | string | undefined {
  if (typeof navigator === "undefined") return undefined;
  return navigator.languages?.length ? navigator.languages : navigator.language;
}

/** Last language setting seen on this device: the first paint uses it until the configuration has answered. */
function readSetting(): LanguageSetting {
  try {
    const v = localStorage.getItem(KEY);
    return isLanguageSetting(v) ? v : DEFAULT_LANGUAGE;
  } catch {
    return DEFAULT_LANGUAGE;
  }
}

let setting: LanguageSetting = readSetting();
let current: Locale = resolveLocale(setting, browserLanguages());
applyToDocument(current);

function applyToDocument(l: Locale): void {
  if (typeof document !== "undefined") document.documentElement.lang = l;
}

/** The language the interface is shown in right now. */
export const getLocale = (): Locale => current;
export const getLanguageSetting = (): LanguageSetting => setting;

/** Applies a setting: stores it on the device, updates `<html lang>` and the formatters, wakes subscribers. */
export function setLanguageSetting(next: LanguageSetting): void {
  setting = next;
  try {
    localStorage.setItem(KEY, next);
  } catch {
    /* storage unavailable: the choice lasts until the page is reloaded */
  }
  current = resolveLocale(next, browserLanguages());
  applyToDocument(current);
  for (const l of listeners) l();
}

/**
 * BCP 47 tag the `Intl` formatters use: Italian is `it-IT`; English keeps the region of the
 * browser when it is an English one (so dates follow the person's habits), else `en-GB`.
 */
export function intlTag(l: Locale = current): string {
  if (l === "it") return "it-IT";
  const nav = typeof navigator !== "undefined" ? navigator.language : "";
  return /^en-[A-Za-z]{2}$/.test(nav) ? nav : "en-GB";
}

/** Forces the active language without touching storage (tests and the demo transport). */
export function forceLocale(l: Locale): void {
  setting = l;
  current = l;
  applyToDocument(l);
  for (const fn of listeners) fn();
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

/** The active language; the component re-renders when it changes. */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribe, () => current, () => "it");
}

/** The stored setting (`auto`, `it`, `en`); the component re-renders when it changes. */
export function useLanguageSetting(): LanguageSetting {
  return useSyncExternalStore(subscribe, () => setting, () => DEFAULT_LANGUAGE);
}
