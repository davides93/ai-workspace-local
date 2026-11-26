import { t, tList } from "../../../shared/i18n";
const SUGGESTIONS = tList(["chat.riordina_i_pdf_in_downloads", "chat.confronta_i_preventivi_nella_cartella", "chat.apri_il_sito_del_comune"]);

/** What the simple chat offers to start from: tasks of everyday life, each with a one-line hint and the text it puts in the message box. */
const TASKS = [
  { id: "posta", title: "chat.compito_posta_titolo", hint: "chat.compito_posta_dettaglio", text: "chat.compito_posta_testo" },
  { id: "giornata", title: "chat.compito_giornata_titolo", hint: "chat.compito_giornata_dettaglio", text: "chat.compito_giornata_testo" },
  { id: "documenti", title: "chat.compito_documenti_titolo", hint: "chat.compito_documenti_dettaglio", text: "chat.compito_documenti_testo" },
  { id: "bozza", title: "chat.compito_bozza_titolo", hint: "chat.compito_bozza_dettaglio", text: "chat.compito_bozza_testo" },
  { id: "pdf", title: "chat.compito_pdf_titolo", hint: "chat.compito_pdf_dettaglio", text: "chat.riordina_i_pdf_in_downloads" },
  { id: "memoria", title: "chat.suggerimento_cosa_sai_di_me", hint: "chat.compito_memoria_dettaglio", text: "chat.suggerimento_cosa_sai_di_me" },
] as const;

/** A session with no messages yet: what the agent does and a few prompts to start from (`simple`: tasks of everyday life as cards). */
export function ChatEmpty({ onPick, simple = false }: { onPick: (text: string) => void; simple?: boolean }) {
  if (simple) {
    return (
      <div className="ws-hero">
        <h2>{t("chat.compiti_cosa_vuoi_fare")}</h2>
        <ul className="ws-tasks" aria-label={t("chat.idee_per_iniziare")}>
          {TASKS.map((k) => (
            <li key={k.id}>
              <button type="button" className="ws-task" onClick={() => onPick(t(k.text))}>
                <span className="ws-task-title">{t(k.title)}</span>
                <span className="ws-task-hint">{t(k.hint)}</span>
              </button>
            </li>
          ))}
        </ul>
        <p className="muted ws-tasks-note">{t("chat.compiti_nota")}</p>
      </div>
    );
  }
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
