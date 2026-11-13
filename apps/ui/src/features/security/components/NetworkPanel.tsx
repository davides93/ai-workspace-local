import { currentLocaleTag } from "../../../shared/lib/format";
import { useApi } from "../../../shared/api/context";
import { toast } from "../../../shared/lib/toast";
import { useAction } from "../../../shared/lib/useAction";
import { Button, CopyField, KeyValue, Panel, Segmented } from "../../../shared/ui";
import { useSetConfig } from "../../settings/api";
import { useTlsStatus } from "../api";

import { t, tNode } from "../../../shared/i18n";
const MODES = [
  { id: "off", label: "Nessuna" },
  { id: "self_signed", label: "Certificato di AIW" },
  { id: "files", label: "Il mio certificato" },
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
            ["Ascolto", s.lan ? `tutta la rete locale, porta ${s.port}` : `solo questo computer (127.0.0.1), porta ${s.port}`],
            ["Cifratura", s.active ? "attiva (HTTPS)" : s.mode === "off" ? "spenta (HTTP)" : "scelta, ma non ancora attiva: riavvia il servizio"],
          ]}
        />
        <p className="faint text-sm">L&apos;indirizzo e la porta si cambiano in Impostazioni (accesso dalla LAN, porta API). La cifratura vale solo con l&apos;accesso dalla LAN acceso, e ha effetto al riavvio del servizio.</p>
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
            Quando accoppi un altro computer o un telefono con l&apos;app di AI Workspace, controlla che questa impronta sia la stessa che compare sull&apos;altro dispositivo (il QR di pairing la include): da quel momento si fida solo di questo certificato.
          </p>
          {s.names.length > 0 && <p className="faint text-xs">{t("security.valido_per", { p: s.names.join(", "), p2: s.expires_at ? ` · rinnovato da solo prima del ${new Date(s.expires_at).toLocaleDateString(currentLocaleTag())}` : "" })}</p>}
          {s.ca_available && (
            <>
              <Button icon="download" disabled={download.pending} onClick={() => void download.run()}>{t("security.scarica_il_certificato")}</Button>
              <p className="faint text-sm">
                <b>{t("security.browser_su_un_altro_dispositivo")}</b> il browser non può «fissare» un certificato, quindi mostrerà un avviso finché non installi questo certificato come autorità attendibile su quel dispositivo (e confronti l&apos;impronta qui sopra). Senza questo passaggio non c&apos;è alcuna garanzia che stai parlando con il tuo computer.
              </p>
            </>
          )}
        </Panel>
      )}
    </div>
  );
}
