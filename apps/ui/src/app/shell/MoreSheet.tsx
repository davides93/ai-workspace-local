import { useTheme } from "../../shared/lib/theme";
import { UI_MODE_LABEL, UI_MODES } from "../../shared/lib/uiMode";
import { Icon, Segmented, Sheet } from "../../shared/ui";
import { Link } from "../router";
import { hasNavigation, navGroups, routeLabel } from "../navigation";
import { GROUPS, ROUTES, routeHref, type RouteId } from "../routes";
import { useUiMode } from "../useUiMode";
import { InboxBadge } from "../../features/inbox/components/InboxBadge";
import { LanguageSelect } from "../LanguageSelect";
import { PrivacyPanel } from "../../features/settings/components/PrivacyPanel";
import { DeviceChip } from "./DeviceChip";
import { KillSwitch } from "./KillSwitch";

import { t } from "../../shared/i18n";
/**
 * "Altro" sheet of the phone tab bar: every section, the current device, theme and the kill switch.
 * In the simple interface it is the menu of the chat (title «Menu», no grid of sections: there is one place to be):
 * device, search, mode, language, theme and the kill switch.
 */
export function MoreSheet({ current, onClose, onOpenPalette }: { current?: RouteId; onClose: () => void; onOpenPalette: () => void }) {
  const [theme, setTheme] = useTheme();
  const [mode, setMode] = useUiMode();
  const sections = hasNavigation(ROUTES, mode);
  return (
    <Sheet title={sections ? t("shell.altre_sezioni") : t("shell.menu")} onClose={onClose} footer={<KillSwitch variant="sheet" />}>
      <div className="stack stack-lg more-sheet">
        <DeviceChip onNavigate={onClose} />
        <button type="button" className="rail-search" onClick={() => { onClose(); onOpenPalette(); }}>
          <Icon name="search" size={16} />
          <span>{t("shell.cerca_e_comandi_2")}</span>
        </button>
        {sections && navGroups(GROUPS, ROUTES, mode).map(({ group, label, routes }) => (
          <div key={group.id} className="stack stack-sm">
            {label && <span className="eyebrow">{label}</span>}
            <div className="more-grid">
              {routes.map((r) => (
                <Link key={r.id} to={routeHref(r)} className="more-item" aria-current={current === r.id ? "page" : undefined} onClick={onClose}>
                  <Icon name={r.icon} />
                  <span>{routeLabel(r, mode)}</span>
                  {r.id === "inbox" && <InboxBadge />}
                </Link>
              ))}
            </div>
          </div>
        ))}
        <PrivacyPanel />
        <div className="stack stack-sm">
          <span className="eyebrow">{t("shell.modalita_dell_interfaccia")}</span>
          <Segmented label={t("shell.modalita_dell_interfaccia")} value={mode} onChange={setMode} options={UI_MODES.map((m) => ({ id: m, label: UI_MODE_LABEL[m] }))} />
        </div>
        <div className="stack stack-sm">
          <span className="eyebrow">{t("common.language")}</span>
          <LanguageSelect />
        </div>
        <div className="stack stack-sm">
          <span className="eyebrow">{t("shell.tema")}</span>
          <Segmented label={t("shell.tema")} value={theme} onChange={setTheme} options={[{ id: "system", label: t("shell.sistema") }, { id: "light", label: t("shell.chiaro") }, { id: "dark", label: t("shell.scuro") }]} />
        </div>
      </div>
    </Sheet>
  );
}
