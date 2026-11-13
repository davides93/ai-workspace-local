import { currentLocaleTag } from "../../../shared/lib/format";
import { useApi } from "../../../shared/api/context";
import { toast } from "../../../shared/lib/toast";
import { useAction } from "../../../shared/lib/useAction";
import { Button, CopyField, KeyValue, Panel, Segmented } from "../../../shared/ui";
import { useSetConfig } from "../../settings/api";
import { useTlsStatus } from "../api";

import { t, tNode, t as tr } from "../../../shared/i18n";
const MODES = [
  { id: "off", get label() { return tr("security.nessuna"); } },
  { id: "self_signed", get label() { return tr("security.certificato_di_aiw"); } },
  { id: "files", get label() { return tr("security.il_mio_certificato"); } },
] as const;

/** Where the server listens, whether it is encrypted, and what to compare when pairing. */
export function NetworkPanel() {
  const api = useApi();
  const status = useTlsStatus();
  const setConfig = useSetConfig();
  const s = status.data;
  const download = useAction(async () => {
    const { pem } = await api.tlsCa();
    const url = URL.createObjectURL(new Blob([pem], { type: "application/x-pem-file" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = "aiw-ca.pem";
    a.click();
    URL.revokeObjectURL(url);
    toast(t("security.certificato_scaricato"));
  });
  if (!s) return <p className="faint">{t("security.caricamento")}</p>;
  return (
    <div className="stack">
      {s.warning && (
        <div className="notice warn" role="alert">
          {tNode("security.questo_computer_e_raggiungibile_dalla", { b: <b>{t("security.rete_in_chiaro")}</b> })}</div>
      )}
      {s.error && <div className="notice danger" role="alert">{t("security.tls_non_disponibile", { error: s.error })}</div>}
      <Panel icon="shield" title={t("security.rete")}>
        <KeyValue
          items={[
            [tr("security.ascolto"), s.lan ? tr("security.tutta_la_rete_locale_porta", { port: s.port }) : tr("security.solo_questo_computer_127_0", { port: s.port })],
            [tr("security.cifratura"), s.active ? tr("security.attiva_https") : s.mode === "off" ? tr("security.spenta_http") : tr("security.scelta_ma_non_ancora_attiva")],
          ]}
        />
        <p className="faint text-sm">{t("security.network_address_note")}</p>
        <Segmented
          label={t("security.cifratura_della_rete")} value={s.mode} options={MODES.map((m) => ({ id: m.id, label: m.label }))}
          onChange={(mode) => setConfig.mutate({ layer: "global", key: "server.tls", value: mode, title: t("security.cifratura_della_rete") }, { onSuccess: () => void status.refetch() })}
        />
        {s.mode === "files" && <p className="faint text-sm">{t("security.indica_il_certificato_e_la")}</p>}
      </Panel>
      {s.fingerprint && (
        <Panel icon="key" title={t("security.impronta_del_certificato")}>
          <CopyField label={s.fingerprint_of === "ca" ? t("security.impronta_sha_256_dell_autorita") : t("security.impronta_sha_256_del_certificato")} value={s.fingerprint} />
          <p className="faint text-sm">
            {t("security.fingerprint_pairing_note")}
          </p>
          {s.names.length > 0 && <p className="faint text-xs">{t(s.expires_at ? "security.valid_for_renewed" : "security.valid_for", { names: s.names.join(", "), date: s.expires_at ? new Date(s.expires_at).toLocaleDateString(currentLocaleTag()) : "" })}</p>}
          {s.ca_available && (
            <>
              <Button icon="download" disabled={download.pending} onClick={() => void download.run()}>{t("security.scarica_il_certificato")}</Button>
              <p className="faint text-sm">
                <b>{t("security.browser_su_un_altro_dispositivo")}</b> {t("security.browser_cannot_pin_note")}
              </p>
            </>
          )}
        </Panel>
      )}
    </div>
  );
}
