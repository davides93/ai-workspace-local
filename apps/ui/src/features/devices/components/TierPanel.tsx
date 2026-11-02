import type { DeviceTier } from "../../../shared/api/types";
import { cx } from "../../../shared/lib/cx";
import { Chip, Panel } from "../../../shared/ui";
import { TIER_INFO, TIER_ORDER } from "../model/hardware";

/** The class of this device (XS–XL) and what each class can run. */
export function TierPanel({ tier }: { tier: DeviceTier }) {
  return (
    <Panel>
      <div className="stack">
        <span className="eyebrow">Classe del dispositivo</span>
        <div className="row row-baseline">
          <span className="tier" aria-hidden="true">{tier.toUpperCase()}</span>
          <b>{TIER_INFO[tier].label}</b>
        </div>
        <p className="muted">{TIER_INFO[tier].models}</p>
        <ul className="stack stack-sm dev-tiers" aria-label="Classi dei dispositivi">
          {TIER_ORDER.map((t) => (
            <li key={t} className="row nowrap" aria-current={t === tier ? "true" : undefined}>
              <Chip hot={t === tier} className="tier-chip">{t.toUpperCase()}</Chip>
              <span className={cx("text-sm", t !== tier && "faint")}>{TIER_INFO[t].models}</span>
            </li>
          ))}
        </ul>
      </div>
    </Panel>
  );
}
