// Setup of the component tests (`*.test.tsx`, jsdom). The logic tests run in `node` and never load this file.
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import { forceLocale } from "../shared/i18n/locale";

// React wants to know that updates happen inside a test (act); Testing Library sets it too, but only when it is imported first.
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// What jsdom does not implement. The app guards most of these (`window.matchMedia?.`, `typeof ResizeObserver`), the
// stubs keep the behaviour a desktop browser would have in the places that do not.
if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false, media: query, onchange: null,
      addEventListener: () => {}, removeEventListener: () => {}, addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
    }) as MediaQueryList;
}
Element.prototype.scrollIntoView ??= () => {};
Element.prototype.scrollTo ??= () => {};
HTMLElement.prototype.scrollTo ??= () => {};
window.scrollTo = () => {};
HTMLDialogElement.prototype.showModal ??= function showModal(this: HTMLDialogElement) { this.setAttribute("open", ""); };
HTMLDialogElement.prototype.close ??= function close(this: HTMLDialogElement) { this.removeAttribute("open"); };
URL.createObjectURL ??= () => "blob:test";
URL.revokeObjectURL ??= () => {};

// The configuration says `ui.language: auto`, which follows the browser: jsdom says English, the tests assert the Italian wording
// (like the Playwright projects, which run with the it-IT locale). A test of the English interface sets `navigator.languages` itself.
export const setBrowserLanguages = (languages: string[]): void => {
  Object.defineProperty(navigator, "languages", { value: languages, configurable: true });
  Object.defineProperty(navigator, "language", { value: languages[0], configurable: true });
};

beforeEach(() => {
  setBrowserLanguages(["it-IT", "it"]);
  // Every test starts as a new device: nothing stored, the Italian wording the tests assert, the address at the root.
  localStorage.clear();
  sessionStorage.clear();
  forceLocale("it");
  document.documentElement.removeAttribute("data-theme");
  location.hash = "";
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});
