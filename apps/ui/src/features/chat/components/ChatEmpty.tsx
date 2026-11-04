const SUGGESTIONS = [
  "Riordina i PDF in Downloads per anno e crea un indice",
  "Confronta i preventivi nella cartella Fornitori",
  "Apri il sito del comune e trova gli orari dell'anagrafe",
];

/** A session with no messages yet: what the agent does and a few prompts to start from. */
export function ChatEmpty({ onPick }: { onPick: (text: string) => void }) {
  return (
    <div className="ws-hero">
      <h2>Cosa facciamo, in locale?</h2>
      <p className="muted">L'agente lavora con i modelli di questo dispositivo e chiede conferma prima di modificare file o eseguire comandi.</p>
      <ul className="ws-suggestions" aria-label="Idee per iniziare">
        {SUGGESTIONS.map((s) => (
          <li key={s}><button type="button" className="btn" onClick={() => onPick(s)}>{s}</button></li>
        ))}
      </ul>
    </div>
  );
}
