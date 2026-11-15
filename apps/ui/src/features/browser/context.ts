import { createContext, useContext } from "react";

/** What the cards of the conversation need to know about the panel of the browser, which the chat opens and closes. */
export interface BrowserUi {
  /** The panel is open: the one place that shows the live view at a time (it polls frames, and takes the keys). */
  panelOpen: boolean;
  openPanel: () => void;
}

export const BrowserUiContext = createContext<BrowserUi>({ panelOpen: false, openPanel: () => {} });

export const useBrowserUi = (): BrowserUi => useContext(BrowserUiContext);
