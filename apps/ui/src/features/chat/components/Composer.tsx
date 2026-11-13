import { useEffect, useId, useLayoutEffect, useRef, useState, type ClipboardEvent, type CSSProperties, type DragEvent } from "react";
import { Button, IconButton, Select } from "../../../shared/ui";
import type { ThinkingMode } from "../../../shared/api/types";
import { isThinkingMode, THINKING_HINT, THINKING_LABEL, THINKING_MODES } from "../../../shared/lib/thinking";
import type { VoiceController } from "../useVoice";
import type { PendingFile } from "../model/pending";
import { PendingAttachments } from "./Attachments";
import { VoiceBar } from "./VoiceBar";

import { t, t as tr, tNode as trNode } from "../../../shared/i18n";
const MAX_HEIGHT = 200;

const finePointer = () => typeof window !== "undefined" && !!window.matchMedia?.("(pointer: fine)").matches;

/**
 * Message box. Enter sends, Shift+Enter adds a line, Esc stops a run that is going on. The focus
 * ring is drawn on the box (`:focus-within`), the textarea grows with its text, and the box takes
 * the focus back when a session opens or a run ends (on touch screens it never pops the keyboard).
 */
export function Composer({ value, onChange, onSend, onStop, running, disabled, placeholder, modelLabel, focusKey, voice, thinking, attachments, onMcpPrompt }: {
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
  /** Dictation and voice conversation; without it the box has no microphone. */
  voice?: VoiceController;
  /** "Ragionamento" of the conversation; only given when the model can reason. */
  thinking?: { value: ThinkingMode; onChange: (mode: ThinkingMode) => void };
  /** Files for the message: chosen with the paperclip, dropped on the box or pasted. Without it the box takes text only. */
  attachments?: { items: PendingFile[]; onAdd: (files: File[], opts?: { pasted?: boolean }) => void; onRemove: (key: string) => void };
  /** Opens the «Prompt MCP» picker (developer mode, when a connected server has prompts). */
  onMcpPrompt?: () => void;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const hintId = useId();
  const wasRunning = useRef(false);
  const picker = useRef<HTMLInputElement>(null);
  const [dragging, setDragging] = useState(false);

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

  const converse = !!voice && voice.convo !== "off";
  const hasFiles = (attachments?.items.length ?? 0) > 0;
  const canSend = !disabled && !running && (value.trim() !== "" || hasFiles);

  const carriesFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes("Files");
  const onDragOver = (e: DragEvent) => {
    if (!attachments || disabled || !carriesFiles(e)) return;
    e.preventDefault();
    setDragging(true);
  };
  const onDrop = (e: DragEvent) => {
    if (!attachments || !carriesFiles(e)) return;
    e.preventDefault();
    setDragging(false);
    if (!disabled) attachments.onAdd(Array.from(e.dataTransfer.files));
  };
  // Pasted pictures (a screenshot) become attachments; pasted text goes in as usual.
  const onPaste = (e: ClipboardEvent) => {
    const files = Array.from(e.clipboardData?.files ?? []);
    if (!attachments || disabled || files.length === 0) return;
    if (!e.clipboardData.getData("text/plain")) e.preventDefault();
    attachments.onAdd(files, { pasted: true });
  };
  return (
    <div
      className={`ws-composer${dragging ? " is-dragging" : ""}`} ref={root}
      onDragOver={onDragOver} onDragLeave={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false); }} onDrop={onDrop}
    >
      {attachments && <PendingAttachments items={attachments.items} onRemove={attachments.onRemove} />}
      {voice && voice.convo !== "off" && (
        <VoiceBar convo={voice.convo} level={voice.level} waiting={voice.waitingApproval} backend={voice.status?.backend} onStop={voice.stopConvo} />
      )}
      <div className="ws-composer-box">
        {attachments && (
          <>
            <input
              ref={picker} type="file" multiple hidden tabIndex={-1} aria-hidden="true"
              onChange={(e) => {
                attachments.onAdd(Array.from(e.target.files ?? []));
                e.target.value = ""; // the same file can be chosen again
              }}
            />
            <IconButton
              icon="paperclip" className="ws-attach" aria-label={t("chat.allega_un_file_o_un")}
              title={t("chat.allega_un_file_o_un_2")} disabled={disabled} onClick={() => picker.current?.click()}
            />
          </>
        )}
        {onMcpPrompt && (
          <IconButton icon="plug" aria-label={t("chat.prompt_mcp")} title={t("chat.inserisci_un_prompt_di_un")} disabled={disabled} onClick={onMcpPrompt} />
        )}
        <textarea
          ref={ref}
          id="composer"
          aria-label={t("chat.messaggio")}
          aria-describedby={hintId}
          rows={1}
          placeholder={placeholder}
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
          onPaste={onPaste}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return;
            if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
              e.preventDefault();
              if (canSend) onSend();
            } else if (e.key === "Escape" && running && !converse) {
              e.preventDefault();
              onStop();
            }
          }}
        />
        {voice && voice.supported && (
          <>
            <IconButton
              icon={voice.dictation === "busy" ? "more" : "mic"}
              className={`ws-mic ${voice.dictation}`}
              aria-label={voice.dictation === "rec" ? t("chat.ferma_la_dettatura") : t("chat.detta_il_messaggio")}
              aria-pressed={voice.dictation === "rec"}
              title={!voice.available ? t("chat.voce_non_disponibile", { p: voice.status?.detail ?? "" }) : voice.dictation === "rec" ? t("chat.ferma_la_dettatura") : t("chat.detta", { p: voice.status?.backend ?? "trascrizione locale" })}
              disabled={disabled || voice.dictation === "busy" || converse}
              style={voice.dictation === "rec" ? { "--mic-level": voice.level } as CSSProperties : undefined}
              onClick={voice.toggleDictation}
            />
            <IconButton
              icon="wave"
              className={`ws-talk ${converse ? "on" : ""}`}
              aria-label={converse ? t("chat.termina_la_conversazione_a_voce") : t("chat.conversazione_a_voce")}
              aria-pressed={converse}
              title={converse ? t("chat.termina_la_conversazione_a_voce_2") : t("chat.conversazione_a_voce_parli_l")}
              disabled={disabled || voice.dictation !== "off"}
              onClick={voice.toggleConvo}
            />
          </>
        )}
        {running ? (
          <Button variant="danger" icon="stop" onClick={onStop} aria-keyshortcuts="Escape">{t("chat.ferma")}</Button>
        ) : (
          <Button variant="primary" icon="send" onClick={onSend} disabled={!canSend}>{t("chat.invia")}</Button>
        )}
      </div>
      <div className="ws-composer-meta">
        {thinking && (
          <label className="ws-think" title={THINKING_HINT[thinking.value]}>
            <span>{tr("chat.ragionamento_2")}</span>
            <Select
              aria-label={t("chat.ragionamento")} value={thinking.value}
              onChange={(e) => isThinkingMode(e.target.value) && thinking.onChange(e.target.value)}
            >
              {THINKING_MODES.map((m) => <option key={m} value={m}>{THINKING_LABEL[m]}</option>)}
            </Select>
          </label>
        )}
        {modelLabel && <span className="mono ws-composer-model" title={t("chat.runtime_e_modello_della_sessione")}>{modelLabel}</span>}
        <span id={hintId} className="ws-composer-hint">
          {running ? (
            <>{trNode("chat.risposta_in_corso_per_fermarla", { kbd: <kbd>{tr("chat.esc")}</kbd> })}</>
          ) : (
            <>{trNode("chat.per_inviare_per_andare_a", { kbd: <kbd>{tr("chat.invio")}</kbd>, kbd2: <kbd>{tr("chat.maiusc")}</kbd>, kbd3: <kbd>{tr("chat.invio")}</kbd> })}</>
          )}
        </span>
      </div>
    </div>
  );
}
