import { useTheme } from "../../shared/lib/theme";
import { UI_MODE_LABEL, UI_MODES } from "../../shared/lib/uiMode";
import { Icon, Segmented, Sheet } from "../../shared/ui";
import { Link } from "../router";
import { navGroups, routeLabel } from "../navigation";
import { GROUPS, ROUTES, routeHref, type RouteId } from "../routes";
import { useUiMode } from "../useUiMode";
import { DeviceChip } from "./DeviceChip";
import { KillSwitch } from "./KillSwitch";

/** "Altro" sheet of the phone tab bar: every section, the current device, theme and the kill switch. */
export function MoreSheet({ current, onClose, onOpenPalette }: { current?: RouteId; onClose: () => void; onOpenPalette: () => void }) {
  const [theme, setTheme] = useTheme();
  const [mode, setMode] = useUiMode();
  return (
    <Sheet title="Altre sezioni" onClose={onClose} footer={<KillSwitch variant="sheet" />}>
      <div className="stack stack-lg more-sheet">
        <DeviceChip onNavigate={onClose} />
        <button type="button" className="rail-search" onClick={() => { onClose(); onOpenPalette(); }}>
          <Icon name="search" size={16} />
          <span>Cerca e comandi…</span>
        </button>
        {navGroups(GROUPS, ROUTES, mode).map(({ group, label, routes }) => (
          <div key={group.id} className="stack stack-sm">
            {label && <span className="eyebrow">{label}</span>}
            <div className="more-grid">
              {routes.map((r) => (
                <Link key={r.id} to={routeHref(r)} className="more-item" aria-current={current === r.id ? "page" : undefined} onClick={onClose}>
                  <Icon name={r.icon} />
                  <span>{routeLabel(r, mode)}</span>
                </Link>
              ))}
            </div>
          </div>
        ))}
        <div className="stack stack-sm">
          <span className="eyebrow">Modalità dell'interfaccia</span>
          <Segmented label="Modalità dell'interfaccia" value={mode} onChange={setMode} options={UI_MODES.map((m) => ({ id: m, label: UI_MODE_LABEL[m] }))} />
        </div>
        <div className="stack stack-sm">
          <span className="eyebrow">Tema</span>
          <Segmented label="Tema" value={theme} onChange={setTheme} options={[{ id: "system", label: "Sistema" }, { id: "light", label: "Chiaro" }, { id: "dark", label: "Scuro" }]} />
        </div>
      </div>
    </Sheet>
  );
}
