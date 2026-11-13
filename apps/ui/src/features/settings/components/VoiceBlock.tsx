import { useState } from "react";
import { Button, Pill, Select } from "../../../shared/ui";
import { useReader } from "../../chat/useReader";
import { useVoiceStatus } from "../../chat/api";
import { engineLine, languageName, nextPicks, pickerLanguages, SAMPLE, savedPick, voicesFor } from "../model/voicePicker";

import { t } from "../../../shared/i18n";
/** «Voce» section: what reads the replies aloud and a voice for each language, with a button to listen to it. */
export function VoiceBlock({ picks, onPick }: {
  /** The current value of `voice.tts.voice`. */
  picks: unknown;
  /** Writes `voice.tts.voice` (`null` removes it). */
  onPick: (value: Record<string, string> | null) => void;
}) {
  const status = useVoiceStatus();
  const { read, stop, speaking } = useReader();
  const [testing, setTesting] = useState<string | null>(null);
  const tts = status.data?.tts;
  if (!tts) return null;
  const line = engineLine(tts);
  const test = async (language: string) => {
    if (testing) return stop();
    setTesting(language);
    try {
      await read(SAMPLE[language] ?? SAMPLE.en, { language });
    } finally {
      setTesting(null);
    }
  };
  return (
    <div className="panel set-voice" data-testid="voice-block">
      <h3 className="set-bg-title">{t("settings.voce_che_legge_le_risposte")}</h3>
      <p className="set-bg-line">
        <Pill tone={line.tone}>{tts.neural ? t("settings.neurale") : t("settings.sistema")}</Pill>
        <span>{line.text}</span>
      </p>
      <ul className="set-voice-list">
        {pickerLanguages(tts).map((lang) => {
          const voices = voicesFor(tts, lang);
          const id = `set-voice-${lang}`;
          return (
            <li key={lang} className="set-voice-row">
              <label htmlFor={id} className="set-title">{languageName(lang)}</label>
              <Select
                id={id} value={savedPick(picks, lang) ?? ""} disabled={voices.length === 0}
                onChange={(e) => onPick(nextPicks(picks, lang, e.target.value))}
              >
                <option value="">{voices.length ? `Automatica (${voices.find((v) => v.id === tts.picks[lang])?.name ?? voices[0].name})` : t("settings.nessuna_voce_neurale")}</option>
                {voices.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
              </Select>
              <Button size="sm" icon={testing === lang && speaking ? "stop" : "play"} onClick={() => void test(lang)} disabled={testing !== null && testing !== lang}>
                {testing === lang ? t("settings.ferma") : t("settings.prova")}
              </Button>
            </li>
          );
        })}
      </ul>
      <p className="set-hint">{t("settings.prova_legge_una_frase_con")}</p>
    </div>
  );
}
