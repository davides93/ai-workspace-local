import { useKill } from "../../shared/query/system";
import { Icon } from "../../shared/ui";

/** Emergency stop. Always one tap away: in the rail on desktop, in the "Altro" sheet on phones, in the palette. */
export function KillSwitch({ variant = "rail" }: { variant?: "rail" | "sheet" }) {
  const kill = useKill();
  return (
    <button
      type="button"
      className={`kill ${variant === "sheet" ? "kill-lg" : ""}`}
      onClick={() => kill.mutate()}
      disabled={kill.isPending}
      title="Ferma subito tutti i run, i sidecar avviati dai run e il browser controllato"
      aria-label="Kill switch: ferma tutti i run"
    >
      <Icon name="power" />
      <span className="nav-text">Kill switch</span>
    </button>
  );
}
