import { useKill } from "../../shared/query/system";
import { Icon } from "../../shared/ui";

import { t } from "../../shared/i18n";
/** Emergency stop. Always one tap away: in the rail on desktop, in the "Altro" sheet on phones, in the palette. */
export function KillSwitch({ variant = "rail" }: { variant?: "rail" | "sheet" }) {
  const kill = useKill();
  return (
    <button
      type="button"
      className={`kill ${variant === "sheet" ? "kill-lg" : ""}`}
      onClick={() => kill.mutate()}
      disabled={kill.isPending}
      title={t("shell.interrompe_run_addestramenti_runtime_gestiti")}
      aria-label={t("shell.kill_switch_interrompi_run_e")}
    >
      <Icon name="power" />
      <span className="nav-text">{t("shell.kill_switch")}</span>
    </button>
  );
}
