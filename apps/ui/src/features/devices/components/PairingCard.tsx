import { errorMessage } from "../../../shared/lib/errors";
import { Pill, Panel, CopyButton, KeyValue } from "../../../shared/ui";
import { usePairingQr } from "../api";
import { digitsOf, groupCode } from "../model/mesh";

import { t, tNode, t as tr } from "../../../shared/i18n";
/** This device's side of pairing: the QR for a phone, the 6-digit code and where the device is reachable. */
export function PairingCard({ code, address, discovery }: { code?: string; address: string | null; discovery?: boolean }) {
  const qr = usePairingQr(address);
  return (
    <Panel icon="link" title={t("devices.accoppia_con_questo_dispositivo")}>
      <div className="mesh-me">
        <div className="qr" aria-live="polite">
          {qr.data ? (
            <img src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(qr.data.svg)}`} alt={t("devices.codice_qr_per_accoppiare_un")} width={180} height={180} />
          ) : (
            <span className="dev-qr-note" title={qr.error ? errorMessage(qr.error) : undefined}>
              {!address ? t("devices.il_qr_e_disponibile_con") : qr.error ? t("devices.qr_non_disponibile") : t("devices.genero_il_qr")}
            </span>
          )}
        </div>
        <div className="stack min-w-0">
          <span className="eyebrow">{t("devices.codice_di_pairing")}</span>
          <div className="row nowrap">
            <span className="mono pair-code" aria-label={t("devices.codice_di_pairing_2", { p: code ? digitsOf(code).split("").join(" ") : t("devices.non_disponibile") })}>{code ? groupCode(code) : "—"}</span>
            {code && <CopyButton text={digitsOf(code)} label={t("devices.copia")} />}
          </div>
          <span className="muted text-sm">
            {t("devices.inquadra_il_qr_con_la")}</span>
          <KeyValue items={[
            [tr("devices.indirizzo"), <span className="mono">{address ?? t("devices.solo_locale")}</span>],
            [tr("devices.scoperta"), discovery ? <Pill tone="ok">{t("devices.mdns_attivo")}</Pill> : <Pill>{t("devices.spenta")}</Pill>],
          ]} />
          {!address && (
            <span className="faint text-xs">
              {tNode("devices.attiva_accesso_dalla_lan_nelle", { span: <span className="mono">{t("devices.aiw_serve_lan")}</span> })}</span>
          )}
        </div>
      </div>
    </Panel>
  );
}
