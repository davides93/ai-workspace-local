import type { DiscoveredDevice, PeerDevice } from "../../../shared/api/types";
import { relTime } from "../../../shared/lib/format";
import { Button, Icon, IconButton, Panel, Pill } from "../../../shared/ui";
import { isPaired, OS_LABEL } from "../model/mesh";

import { t, t as tr } from "../../../shared/i18n";
/** Devices found on the LAN by mDNS; "Accoppia" fills the address in the form below. */
export function DiscoveredPanel({ discovered, peers, discoveryOn, refreshing, onRefresh, onChoose, children }: {
  discovered: DiscoveredDevice[];
  peers: PeerDevice[];
  discoveryOn: boolean;
  refreshing: boolean;
  onRefresh: () => void;
  onChoose: (d: DiscoveredDevice) => void;
  /** The pair-by-address form. */
  children: React.ReactNode;
}) {
  return (
    <Panel
      icon="globe" title={t("devices.nella_rete")} padded={false}
      actions={<IconButton aria-label={t("devices.cerca_di_nuovo_nella_rete")} icon="refresh" size={16} onClick={onRefresh} disabled={refreshing} />}
    >
      <ul className="list dev-list" aria-label={t("devices.dispositivi_trovati_nella_rete")}>
        {discovered.map((d) => (
          <li className="list-item" key={d.id}>
            <Icon name="device" />
            <div className="grow">
              <b className="ellipsis block">{d.name}</b>
              <div className="faint mono ellipsis text-xs">{OS_LABEL[d.os] ?? d.os} · {d.tier.toUpperCase()} · {d.address} · {relTime(d.last_seen)}</div>
            </div>
            {isPaired(peers, d) ? <Pill tone="ok">{t("devices.accoppiato")}</Pill> : <Button size="sm" aria-label={tr("devices.accoppia_2", { name: d.name })} onClick={() => onChoose(d)}>{t("devices.accoppia")}</Button>}
          </li>
        ))}
        {discovered.length === 0 && (
          <li className="list-item faint text-sm">
            {discoveryOn ? t("devices.nessun_altro_dispositivo_ai_workspace") : t("devices.la_scoperta_automatica_funziona_con")}
          </li>
        )}
      </ul>
      {children}
    </Panel>
  );
}
