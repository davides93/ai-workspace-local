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
      title="Interrompe run, addestramenti, runtime gestiti e browser"
      aria-label="Kill switch: interrompi run e addestramenti"
    >
      <Icon name="power" />
      <span className="nav-text">Kill switch</span>
    </button>
  );
}
