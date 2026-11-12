import { IconButton } from "../../../shared/ui";
import { useReader } from "../useReader";

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
      aria-label={speaking ? "Ferma la lettura" : "Leggi ad alta voce"}
      title={speaking ? "Ferma la lettura (Esc)" : "Leggi ad alta voce"}
      onClick={() => (speaking ? stop() : void read(text))}
    />
  );
}
