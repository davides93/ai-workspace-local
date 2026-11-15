import { KeyValue, Panel } from "../../../shared/ui";
import { useSecretsAtRest } from "../api";

import { t } from "../../../shared/i18n";

/**
 * Whether the secrets (provider tokens, MCP headers, passwords, channel tokens, the search key) are encrypted on disk, and if not,
 * why. It only says; it never shows a value. Without a keychain the secrets stay as they were and this is where that is said.
 */
export function SecretsPanel() {
  const status = useSecretsAtRest();
  const s = status.data;
  if (!s) return status.isError ? null : <p className="faint">{t("security.caricamento")}</p>;
  const label = {
    active: t("security.segreti_stato_active"),
    off: t("security.segreti_stato_off"),
    unavailable: t("security.segreti_stato_unavailable"),
    key_lost: t("security.segreti_stato_key_lost"),
    not_used: t("security.segreti_stato_not_used"),
  }[s.state];
  // The kind of notice: calm when sealed, a warning when they stay plain (nothing is lost), an alert when some cannot be read.
  const note = s.state === "active" ? "active" : s.state === "off" ? "off" : s.state === "key_lost" ? "key_lost" : "plain";
  const text = {
    active: t("security.segreti_note_active"),
    plain: t("security.segreti_note_plain", { detail: s.detail ?? t("security.segreti_unavailable_unknown") }),
    off: t("security.segreti_note_off"),
    key_lost: t("security.segreti_note_key_lost"),
  }[note];
  const total = s.encrypted + s.plain + s.unreadable;
  return (
    <Panel icon="lock" title={t("security.segreti_su_questo_computer")}>
      <KeyValue
        items={[
          [t("security.segreti_stato"), label],
          [t("security.segreti_portachiavi"), s.backend ?? "—"],
          [t("security.segreti_valori"), total === 0 ? t("security.segreti_nessuno") : t("security.segreti_conteggio", { encrypted: s.encrypted, plain: s.plain, unreadable: s.unreadable })],
        ]}
      />
      <p className={note === "active" ? "faint text-sm" : note === "key_lost" ? "notice danger" : "notice warn"} role={note === "key_lost" ? "alert" : undefined}>{text}</p>
    </Panel>
  );
}
