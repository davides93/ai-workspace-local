import { useRef, type KeyboardEvent } from "react";
import { Chip, Chips, Icon, Pill } from "../../../shared/ui";
import type { ModelProviderInfo } from "../../../shared/api/types";
import { authBadge } from "../model/providers";

/**
 * Where to look for models, as a single-choice group of cards: name, what the provider is, the formats
 * it can supply and the state of its access token. Arrow keys move the selection (roving tabindex),
 * like a radio group; the name is the accessible name and the description its accessible description.
 */
export function ProviderPicker({ providers, value, onChange }: { providers: ModelProviderInfo[]; value: string; onChange: (id: string) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const move = (e: KeyboardEvent, index: number) => {
    const last = providers.length - 1;
    let next = index;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") next = index === last ? 0 : index + 1;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = index === 0 ? last : index - 1;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = last;
    else return;
    e.preventDefault();
    onChange(providers[next].id);
    ref.current?.querySelectorAll<HTMLButtonElement>('[role="radio"]')[next]?.focus();
  };
  return (
    <div className="mh-providers" role="radiogroup" aria-label="Provider dei modelli" ref={ref}>
      {providers.map((p, i) => {
        const selected = p.id === value;
        const badge = authBadge(p);
        const nameId = `prov-${p.id}-name`;
        const descId = `prov-${p.id}-desc`;
        return (
          <button
            key={p.id} type="button" role="radio" aria-checked={selected} tabIndex={selected ? 0 : -1}
            aria-labelledby={nameId} aria-describedby={descId}
            className="mh-provider" onClick={() => onChange(p.id)} onKeyDown={(e) => move(e, i)}
          >
            <span className="mh-provider-name" id={nameId}>
              <Icon name={selected ? "check" : "cloud"} size={15} />
              {p.name}
            </span>
            <span className="mh-provider-desc" id={descId}>{p.description}</span>
            <Chips>{p.formats.filter((f) => f !== "system").map((f) => <Chip key={f}>{f}</Chip>)}</Chips>
            <Pill tone={badge.tone} title={badge.detail}>{p.auth_configured && <Icon name="key" size={12} />}{badge.label}</Pill>
          </button>
        );
      })}
    </div>
  );
}
