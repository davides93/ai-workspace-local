import { t } from "../../../shared/i18n";
const SUGGESTIONS = [
  "Riordina i PDF in Downloads per anno e crea un indice",
  "Confronta i preventivi nella cartella Fornitori",
  "Apri il sito del comune e trova gli orari dell'anagrafe",
];

/** A session with no messages yet: what the agent does and a few prompts to start from. */
export function ChatEmpty({ onPick }: { onPick: (text: string) => void }) {
  return (
    <div className="ws-hero">
      <h2>{t("chat.cosa_facciamo_in_locale")}</h2>
      <p className="muted">{t("chat.l_agente_lavora_con_i")}</p>
      <ul className="ws-suggestions" aria-label={t("chat.idee_per_iniziare")}>
        {SUGGESTIONS.map((s) => (
          <li key={s}><button type="button" className="btn" onClick={() => onPick(s)}>{s}</button></li>
        ))}
      </ul>
    </div>
  );
}
