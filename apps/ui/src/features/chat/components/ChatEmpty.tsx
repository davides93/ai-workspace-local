import { t, tList } from "../../../shared/i18n";
const SUGGESTIONS = tList(["chat.riordina_i_pdf_in_downloads", "chat.confronta_i_preventivi_nella_cartella", "chat.apri_il_sito_del_comune"]);
/** The simple chat is also where the app is looked after: ideas for that come after the ones for work. */
const SIMPLE_SUGGESTIONS = tList(["chat.riordina_i_pdf_in_downloads", "chat.confronta_i_preventivi_nella_cartella", "chat.apri_il_sito_del_comune", "chat.suggerimento_scarica_un_modello", "chat.suggerimento_cosa_sai_di_me", "chat.suggerimento_collega_un_servizio"]);

/** A session with no messages yet: what the agent does and a few prompts to start from (`simple`: also for looking after the app). */
export function ChatEmpty({ onPick, simple = false }: { onPick: (text: string) => void; simple?: boolean }) {
  return (
    <div className="ws-hero">
      <h2>{t("chat.cosa_facciamo_in_locale")}</h2>
      <p className="muted">{t("chat.l_agente_lavora_con_i")}</p>
      <ul className="ws-suggestions" aria-label={t("chat.idee_per_iniziare")}>
        {(simple ? SIMPLE_SUGGESTIONS : SUGGESTIONS).map((s) => (
          <li key={s}><button type="button" className="btn" onClick={() => onPick(s)}>{s}</button></li>
        ))}
      </ul>
    </div>
  );
}
