import type { ReactNode } from "react";
import { Chip, Led, Pill } from "../../../shared/ui";
import type { RuntimeCompat } from "../../../shared/api/types";
import { FIT, isFit } from "../model/fit";
import { originInfo } from "../model/origin";
import { splitRuntimes } from "../model/variants";

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
    return <p className="mh-note">Nessun runtime dichiara di poter aprire questo formato.</p>;
  }
  const { available, unavailable } = splitRuntimes(runtimes);
  return (
    <div className="mh-runtimes">
      <ChipRow label="Runtime compatibili">
        {available.map((r) => (
          <Chip key={r.id} hot title="Pronto su questo device">
            <Led tone="ok" small />
            {r.name}
          </Chip>
        ))}
        {unavailable.map((r) => (
          <Chip key={r.id} className="mh-chip-off" title={r.reason || "Non disponibile su questo device"}>
            {r.name}
            <span className="sr-only">: non disponibile{r.reason ? `, ${r.reason}` : ""}</span>
          </Chip>
        ))}
      </ChipRow>
      {explain && available.length === 0 && (
        <p className="mh-note">
          Nessun runtime pronto su questo device:{" "}
          {unavailable.map((r) => `${r.name}${r.reason ? ` (${r.reason})` : ""}`).join(", ")}.
        </p>
      )}
    </div>
  );
}

/** Capabilities (chat, tools, vision…) as chips. */
export function CapabilityChips({ items }: { items: string[] }) {
  if (items.length === 0) return null;
  return <ChipRow label="Capacità">{items.map((c) => <Chip key={c}>{c}</Chip>)}</ChipRow>;
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
