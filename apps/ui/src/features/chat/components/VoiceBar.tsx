import { Button } from "../../../shared/ui";
import { CONVO_LABEL, type Convo } from "../model/voiceReply";

import { t } from "../../../shared/i18n";
/** Status strip of the voice conversation: what the agent is doing now, and how to end it. */
export function VoiceBar({ convo, level, waiting, backend, onStop }: {
  convo: Exclude<Convo, "off">;
  /** Microphone level 0..1. */
  level: number;
  /** The agent waits for a confirmation. */
  waiting: boolean;
  backend?: string | null;
  onStop: () => void;
}) {
  return (
    <div className={`ws-voice-bar ${convo} ${waiting ? "waiting" : ""}`} role="status" aria-live="polite">
      <span className="ws-voice-orb" aria-hidden="true" style={{ transform: `scale(${1 + level * 0.9})` }} />
      <div className="ws-voice-text">
        <b>{waiting ? t("chat.serve_la_tua_conferma") : CONVO_LABEL[convo]}</b>
        <div className="faint ellipsis ws-voice-hint">
          {waiting ? t("chat.tocca_consenti_o_nega_per") : convo === "listening" ? t("chat.parla_pure_invio_quando_fai") : backend ?? ""}
        </div>
      </div>
      <Button size="sm" onClick={onStop}>{t("chat.termina")}</Button>
    </div>
  );
}
