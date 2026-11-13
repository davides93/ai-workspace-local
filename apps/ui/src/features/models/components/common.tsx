import type { ReactNode } from "react";
import { navigate } from "../../../app/router";
import { Button, Chip, Led, Pill } from "../../../shared/ui";
import { useInstallRuntime, useRuntimes } from "../../runtimes/api";
import type { RuntimeCompat } from "../../../shared/api/types";
import { FIT, isFit } from "../model/fit";
import { originInfo } from "../model/origin";
import { splitRuntimes } from "../model/variants";

import { t } from "../../../shared/i18n";
/** Row of chips; with a `label` it is a named group (an `aria-label` needs a role to be valid). */
export function ChipRow({ label, children }: { label?: string; children: ReactNode }) {
  return label ? <div className="chips" role="group" aria-label={label}>{children}</div> : <div className="chips">{children}</div>;
}

/** How well a model fits this device; the detail is the tooltip. Nothing when the backend did not compute it. */
export function FitPill({ fit, detail }: { fit: string | undefined; detail?: string }) {
  if (!isFit(fit)) return null;
  return <Pill tone={FIT[fit].tone} title={detail || undefined}>{FIT[fit].label}</Pill>;
}

/** Where the model comes from (AIW, Ollama, LM Studio, Hugging Face cache, manual file). */
export function OriginChip({ origin }: { origin: string | undefined }) {
  const info = originInfo(origin);
  return <Chip hot={origin === "aiw"} title={info.description}>{info.label}</Chip>;
}

/**
 * Runtimes that can run a model: the ones ready on this device are highlighted, the others are dimmed
 * and carry their reason (tooltip, and text for screen readers). `explain` adds the reasons as visible
 * text when nothing is ready, since a tooltip does not exist on touch screens.
 */
export function RuntimeChips({ runtimes, explain = false }: { runtimes: RuntimeCompat[]; explain?: boolean }) {
  if (runtimes.length === 0) {
    return <p className="mh-note">{t("models.nessun_runtime_dichiara_di_poter")}</p>;
  }
  const { available, unavailable } = splitRuntimes(runtimes);
  return (
    <div className="mh-runtimes">
      <ChipRow label={t("models.runtime_compatibili")}>
        {available.map((r) => (
          <Chip key={r.id} hot title={t("models.pronto_su_questo_device")}>
            <Led tone="ok" small />
            {r.name}
          </Chip>
        ))}
        {unavailable.map((r) => (
          <Chip key={r.id} className="mh-chip-off" title={r.reason || t("models.non_disponibile_su_questo_device")}>
            {r.name}
            <span className="sr-only">{t("models.non_disponibile_2", { p: r.reason ? `, ${r.reason}` : "" })}</span>
          </Chip>
        ))}
      </ChipRow>
      {explain && available.length === 0 && (
        <>
          <p className="mh-note">
            {t("models.no_runtime_ready_on_device", { list: unavailable.map((r) => `${r.name}${r.reason ? ` (${r.reason})` : ""}`).join(", ") })}
          </p>
          <InstallRuntimeAction candidates={unavailable} />
        </>
      )}
    </div>
  );
}

/** One click to install, by itself, the first compatible runtime that AIW knows how to install
 *  (the progress is followed in that runtime's page). Nothing when none can be installed here. */
function InstallRuntimeAction({ candidates }: { candidates: RuntimeCompat[] }) {
  const runtimes = useRuntimes();
  const install = useInstallRuntime();
  const target = candidates
    .map((c) => runtimes.data?.find((r) => r.id === c.id))
    .find((r) => r && r.install_auto && r.status === "installable");
  if (!target) return null;
  return (
    <div className="row">
      <Button
        variant="primary" size="sm" icon="download" disabled={install.isPending}
        onClick={() => install.mutate(target, { onSuccess: () => navigate(`/runtimes/${target.id}`) })}
      >
        {install.isPending ? t("models.avvio_l_installazione") : t("models.installa", { name: target.name })}
      </Button>
    </div>
  );
}

/** Capabilities (chat, tools, vision…) as chips. */
export function CapabilityChips({ items }: { items: string[] }) {
  if (items.length === 0) return null;
  return <ChipRow label={t("models.capacita")}>{items.map((c) => <Chip key={c}>{c}</Chip>)}</ChipRow>;
}

/** One labeled readout of an instrument-style row (size, parameters, context window…). */
export function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="mh-stat">
      <dt>{label}</dt>
      <dd className="num">{children}</dd>
    </div>
  );
}

/** Row of readouts; `wide` gives each one more room (longer values such as "2 · 1 in coda"). */
export function Stats({ children, wide = false }: { children: ReactNode; wide?: boolean }) {
  return <dl className={wide ? "mh-stats wide" : "mh-stats"}>{children}</dl>;
}
