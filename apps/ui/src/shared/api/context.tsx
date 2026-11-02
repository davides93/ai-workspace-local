import { createContext, useContext, type ReactNode } from "react";
import type { Transport } from "./transport";

const ApiContext = createContext<Transport | null>(null);

/** Gives the whole tree the transport picked at boot. */
export function ApiProvider({ api, children }: { api: Transport; children: ReactNode }) {
  return <ApiContext.Provider value={api}>{children}</ApiContext.Provider>;
}

/** The active transport (Tauri IPC, HTTP+SSE or the demo mock). */
export function useApi(): Transport {
  const api = useContext(ApiContext);
  if (!api) throw new Error("ApiProvider mancante");
  return api;
}
