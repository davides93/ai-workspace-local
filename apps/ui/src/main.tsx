import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "./shared/styles/tokens.css";
import "./shared/styles/oled.css";
import "./shared/styles/base.css";
import "./shared/styles/components.css";
import "./shared/styles/utilities.css";
import { App } from "./app/App";
import { Providers } from "./app/providers";
import { createTransport, type Transport } from "./shared/api/transport";
import { errorMessage } from "./shared/lib/errors";
import { initTheme } from "./shared/lib/theme";

initTheme();

type BootState = { status: "loading" } | { status: "ready"; api: Transport } | { status: "error"; error: unknown };

/** Boot screen: shows a failed start (blocked import, unreachable backend that throws) with a retry. */
function BootError({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return (
    <div className="boot" role="alert">
      <h1>Non riesco ad avviare il workspace</h1>
      <p className="muted">{errorMessage(error)}</p>
      <button type="button" className="btn primary" onClick={onRetry}>Riprova</button>
    </div>
  );
}

function Boot() {
  const [state, setState] = useState<BootState>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let alive = true;
    setState({ status: "loading" });
    createTransport().then(
      (api) => alive && setState({ status: "ready", api }),
      (error: unknown) => alive && setState({ status: "error", error }),
    );
    return () => {
      alive = false;
    };
  }, [attempt]);

  if (state.status === "error") return <BootError error={state.error} onRetry={() => setAttempt((n) => n + 1)} />;
  if (state.status === "loading") return <div className="boot muted" role="status">Avvio del workspace…</div>;
  return (
    <Providers api={state.api}>
      <App />
    </Providers>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Boot />
  </StrictMode>,
);
