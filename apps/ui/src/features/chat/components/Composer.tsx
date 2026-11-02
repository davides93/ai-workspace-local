import { useEffect, useId, useLayoutEffect, useRef } from "react";
import { Button } from "../../../shared/ui";

const MAX_HEIGHT = 200;

const finePointer = () => typeof window !== "undefined" && !!window.matchMedia?.("(pointer: fine)").matches;

/**
 * Message box. Enter sends, Shift+Enter adds a line, Esc stops a run that is going on. The focus
 * ring is drawn on the box (`:focus-within`), the textarea grows with its text, and the box takes
 * the focus back when a session opens or a run ends (on touch screens it never pops the keyboard).
 */
export function Composer({ value, onChange, onSend, onStop, running, disabled, placeholder, modelLabel, focusKey }: {
  value: string;
  onChange: (v: string) => void;
  onSend: () => void;
  onStop: () => void;
  running: boolean;
  /** No session to write to. */
  disabled: boolean;
  placeholder: string;
  /** `runtime · model` of the session, shown under the box. */
  modelLabel?: string;
  /** Changes when another conversation opens: the box is focused again. */
  focusKey: string | null;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const hintId = useId();
  const wasRunning = useRef(false);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, MAX_HEIGHT)}px`;
  }, [value]);

  useEffect(() => {
    if (!disabled && finePointer()) ref.current?.focus({ preventScroll: true });
  }, [focusKey, disabled]);

  // The Stop button disappears when the run ends: do not leave the focus nowhere.
  useEffect(() => {
    if (wasRunning.current && !running && finePointer()) {
      const a = document.activeElement;
      if (!a || a === document.body || root.current?.contains(a)) ref.current?.focus({ preventScroll: true });
    }
    wasRunning.current = running;
  }, [running]);

  const canSend = !disabled && !running && value.trim() !== "";
  return (
    <div className="ws-composer" ref={root}>
      <div className="ws-composer-box">
        <textarea
          ref={ref}
          id="composer"
          aria-label="Messaggio"
          aria-describedby={hintId}
          rows={1}
          placeholder={placeholder}
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return;
            if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
              e.preventDefault();
              if (canSend) onSend();
            } else if (e.key === "Escape" && running) {
              e.preventDefault();
              onStop();
            }
          }}
        />
        {running ? (
          <Button variant="danger" icon="stop" onClick={onStop} aria-keyshortcuts="Escape">Ferma</Button>
        ) : (
          <Button variant="primary" icon="send" onClick={onSend} disabled={!canSend}>Invia</Button>
        )}
      </div>
      <div className="ws-composer-meta">
        {modelLabel && <span className="mono ws-composer-model" title="Runtime e modello della sessione">{modelLabel}</span>}
        <span id={hintId} className="ws-composer-hint">
          {running ? (
            <>Risposta in corso · <kbd>Esc</kbd> per fermarla</>
          ) : (
            <><kbd>Invio</kbd> per inviare · <kbd>Maiusc</kbd>+<kbd>Invio</kbd> per andare a capo</>
          )}
        </span>
      </div>
    </div>
  );
}
