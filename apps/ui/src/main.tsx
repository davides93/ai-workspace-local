import { Fragment, StrictMode, useEffect, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import "./shared/styles/fonts.css";
import "./shared/styles/tokens.css";
import "./shared/styles/oled.css";
import "./shared/styles/base.css";
import "./shared/styles/components.css";
import "./shared/styles/utilities.css";
import { App } from "./app/App";
import { Providers } from "./app/providers";
import { createTransport, type Transport } from "./shared/api/transport";
import { errorMessage } from "./shared/lib/errors";
import { useLocale } from "./shared/i18n";
import { initTheme } from "./shared/lib/theme";

import { t } from "./shared/i18n";
initTheme();

type BootState = { status: "loading" } | { status: "ready"; api: Transport } | { status: "error"; error: unknown };

/** Boot screen: shows a failed start (blocked import, unreachable backend that throws) with a retry. */
function BootError({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return (
    <div className="boot" role="alert">
      <h1>{t("shell.non_riesco_ad_avviare_il")}</h1>
      <p className="muted">{errorMessage(error)}</p>
      <button type="button" className="btn primary" onClick={onRetry}>{t("shell.riprova")}</button>
    </div>
  );
}

/** Remounts the app when the language changes, so every string and every Intl format is rendered again in the new language. */
function LocaleBoundary({ children }: { children: ReactNode }) {
  const locale = useLocale();
  return <Fragment key={locale}>{children}</Fragment>;
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
  if (state.status === "loading") return <div className="boot muted" role="status">{t("shell.avvio_del_workspace")}</div>;
  return (
    <Providers api={state.api}>
      <LocaleBoundary>
        <App />
      </LocaleBoundary>
    </Providers>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Boot />
  </StrictMode>,
);
