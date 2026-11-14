import { looksLikeSecret } from "../model/secretPattern";

import { t } from "../../../shared/i18n";
/**
 * Warns, while someone types, that what is in the message box looks like a secret: it would stay in the conversation, so it is
 * better entered in the protected field the assistant can ask for. It never blocks sending. The live region is always there
 * (only its text comes and goes), so that assistive technology announces the warning when it appears.
 */
export function SecretWarning({ text, onAskProtectedField }: { text: string; onAskProtectedField: () => void }) {
  const on = looksLikeSecret(text);
  return (
    <div className="ws-secret" data-on={on ? "" : undefined}>
      <p role="status" className="ws-secret-text">{on ? t("chat.segreto_avviso") : ""}</p>
      {on && <button type="button" className="btn sm" onClick={onAskProtectedField}>{t("chat.segreto_chiedi_il_campo")}</button>}
    </div>
  );
}
