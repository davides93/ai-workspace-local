import { useTheme } from "../../shared/lib/theme";
import { Icon, Segmented, Sheet } from "../../shared/ui";
import { Link } from "../router";
import { GROUPS, ROUTES, routeHref, type RouteId } from "../routes";
import { DeviceChip } from "./DeviceChip";
import { KillSwitch } from "./KillSwitch";

/** "Altro" sheet of the phone tab bar: every section, the current device, theme and the kill switch. */
export function MoreSheet({ current, onClose, onOpenPalette }: { current?: RouteId; onClose: () => void; onOpenPalette: () => void }) {
  const [theme, setTheme] = useTheme();
  return (
    <Sheet title="Altre sezioni" onClose={onClose} footer={<KillSwitch variant="sheet" />}>
      <div className="stack stack-lg more-sheet">
        <DeviceChip onNavigate={onClose} />
        <button type="button" className="rail-search" onClick={() => { onClose(); onOpenPalette(); }}>
          <Icon name="search" size={16} />
          <span>Cerca e comandi…</span>
        </button>
        {GROUPS.map((g) => (
          <div key={g.id} className="stack stack-sm">
            {g.label && <span className="eyebrow">{g.label}</span>}
            <div className="more-grid">
              {ROUTES.filter((r) => r.group === g.id).map((r) => (
                <Link key={r.id} to={routeHref(r)} className="more-item" aria-current={current === r.id ? "page" : undefined} onClick={onClose}>
                  <Icon name={r.icon} />
                  <span>{r.label}</span>
                </Link>
              ))}
            </div>
          </div>
        ))}
        <div className="stack stack-sm">
          <span className="eyebrow">Tema</span>
          <Segmented label="Tema" value={theme} onChange={setTheme} options={[{ id: "system", label: "Sistema" }, { id: "light", label: "Chiaro" }, { id: "dark", label: "Scuro" }]} />
        </div>
      </div>
    </Sheet>
  );
}
