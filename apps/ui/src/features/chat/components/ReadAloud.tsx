import { IconButton } from "../../../shared/ui";
import { useReader } from "../useReader";

import { t } from "../../../shared/i18n";
/**
 * Reads a reply aloud (the engine's neural voice, else the system's); the same button stops it,
 * and so does Esc. Only one reply speaks at a time.
 */
export function ReadAloud({ text }: { text: string }) {
  const { read, stop, speaking } = useReader();
  return (
    <IconButton
      icon={speaking ? "stop" : "speaker"}
      size={15}
      aria-pressed={speaking}
      aria-label={speaking ? t("chat.ferma_la_lettura") : t("chat.leggi_ad_alta_voce")}
      title={speaking ? t("chat.ferma_la_lettura_esc") : t("chat.leggi_ad_alta_voce")}
      onClick={() => (speaking ? stop() : void read(text))}
    />
  );
}
