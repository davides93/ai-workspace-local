import { useApi } from "../../shared/api/context";
import { useConnection } from "../../shared/query/system";
import { Button, Icon } from "../../shared/ui";

/** Shown on a demo transport (mockup or fallback), driven by `capabilities.demo`, not by the system mode. */
export function DemoBanner() {
  const api = useApi();
  if (!api.capabilities.demo) return null;
  return (
    <div className="banner" role="status">
      <Icon name="sparkle" size={16} />
      <span>
        <b>Demo interattiva</b> · {api.notice ?? <>dati di esempio; avvia <code>aiw serve</code> o apri l'app desktop per usare i tuoi runtime e modelli</>}
      </span>
    </div>
  );
}

/** The workspace stopped answering (any failure of the system poll except 401). */
export function ConnectionBanner() {
  const { lost, retry, retrying } = useConnection();
  if (!lost) return null;
  return (
    <div className="banner banner-crit" role="alert">
      <Icon name="alert" size={16} />
      <span><b>Connessione al workspace persa</b> · i dati mostrati potrebbero non essere aggiornati.</span>
      <Button size="sm" className="banner-cta" icon="refresh" onClick={retry} disabled={retrying}>{retrying ? "Provo…" : "Riprova"}</Button>
    </div>
  );
}
