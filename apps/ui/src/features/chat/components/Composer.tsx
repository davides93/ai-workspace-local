import { Button } from "../../../shared/ui";
import type { InferenceProfile } from "../../../shared/api/types";

/** Message box. The focus ring is drawn on the box (`:focus-within`), not on the bare textarea. */
export function Composer({ value, onChange, onSend, onStop, running, disabled, placeholder, profile }: {
  value: string;
  onChange: (v: string) => void;
  onSend: () => void;
  onStop: () => void;
  running: boolean;
  disabled: boolean;
  placeholder: string;
  profile: InferenceProfile | null;
}) {
  return (
    <div className="composer">
      <div className="composer-box">
        <textarea
          id="composer"
          aria-label="Messaggio"
          rows={1}
          placeholder={placeholder}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              onSend();
            }
          }}
        />
        {running ? (
          <Button variant="danger" icon="stop" onClick={onStop}>Ferma</Button>
        ) : (
          <Button variant="primary" icon="send" onClick={onSend} disabled={!value.trim() || disabled}>Invia</Button>
        )}
      </div>
      <div className="composer-meta">
        {profile && <span className="mono">{profile.runtime_id} · {profile.model}</span>}
        <span>Invio per mandare · Maiusc+Invio per andare a capo</span>
      </div>
    </div>
  );
}
