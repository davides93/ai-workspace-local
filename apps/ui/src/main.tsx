import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import { createTransport, type Transport } from "./api/transport";
import { AppProvider } from "./state/app";
import { App } from "./App";

function Boot() {
  const [api, setApi] = useState<Transport | null>(null);
  useEffect(() => { createTransport().then(setApi); }, []);
  if (!api) return <div className="page muted">Avvio del workspace…</div>;
  return (
    <AppProvider api={api}>
      <App />
    </AppProvider>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Boot />
  </StrictMode>,
);
