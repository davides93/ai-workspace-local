import { Icon, Select, Sheet } from "../../../shared/ui";
import type { AgentProfile, InferenceProfile, ThinkingMode } from "../../../shared/api/types";
import { isThinkingMode, THINKING_HINT, THINKING_LABEL, THINKING_MODES } from "../../../shared/lib/thinking";
import { modeInfo } from "../model/labels";
import type { VoiceController } from "../useVoice";

import { t } from "../../../shared/i18n";
/**
 * Simple interface: everything about how this conversation works, in one place behind the model button of the header,
 * so the chat itself carries one control, not a row of chips: the model, the approval mode, the reasoning (when the
 * model can reason) and the voice conversation (when the device can do it).
 */
export function ConversationOptions({ profile, role, agent, thinking, voice, onClose }: {
  profile: InferenceProfile | null;
  /** Role of the model («Intelligente»), when known. */
  role?: string;
  agent: AgentProfile | null;
  thinking?: { value: ThinkingMode; onChange: (mode: ThinkingMode) => void };
  voice?: VoiceController;
  onClose: () => void;
}) {
  const mode = modeInfo(agent?.mode);
  const converse = !!voice && voice.convo !== "off";
  return (
    <Sheet title={t("chat.opzioni_conversazione")} onClose={onClose}>
      <div className="stack stack-lg ws-options">
        {profile && (
          <div className="stack stack-sm">
            <span className="eyebrow">{t("chat.opz_modello")}</span>
            <div className="ws-option-row">
              <Icon name="cube" size={16} />
              <b>{role ?? profile.name}</b>
              {profile.model && <span className="mono muted text-xs ellipsis">{profile.model}</span>}
            </div>
            <span className="muted text-sm">{t("chat.opz_modello_hint")}</span>
          </div>
        )}
        <div className="stack stack-sm">
          <span className="eyebrow">{t("chat.approvazione")}</span>
          <div className="ws-option-row"><Icon name="shield" size={16} /><b>{mode.label}</b></div>
          <span className="muted text-sm">{mode.hint || t("chat.opz_approvazione_hint")}</span>
        </div>
        {thinking && (
          <div className="stack stack-sm">
            <label className="eyebrow" htmlFor="ws-opt-thinking">{t("chat.ragionamento")}</label>
            <Select id="ws-opt-thinking" value={thinking.value} onChange={(e) => isThinkingMode(e.target.value) && thinking.onChange(e.target.value)}>
              {THINKING_MODES.map((m) => <option key={m} value={m}>{THINKING_LABEL[m]}</option>)}
            </Select>
            <span className="muted text-sm">{THINKING_HINT[thinking.value]}</span>
          </div>
        )}
        {voice && voice.supported && (
          <div className="stack stack-sm">
            <span className="eyebrow">{t("chat.opz_voce")}</span>
            <button type="button" className="btn" aria-pressed={converse} disabled={voice.dictation !== "off"} onClick={() => { voice.toggleConvo(); if (!converse) onClose(); }}>
              <Icon name="wave" size={16} />
              {converse ? t("chat.opz_voce_ferma") : t("chat.opz_voce_avvia")}
            </button>
            <span className="muted text-sm">{t("chat.opz_voce_hint")}</span>
          </div>
        )}
      </div>
    </Sheet>
  );
}
